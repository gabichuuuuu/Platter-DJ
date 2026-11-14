//! The headphone output: pre-listening, its mix with the master, its volume and the test tone.
//!
//! Behaviour note: `docs/behavior/mixer.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Command, Output};
use platter_testkit::measure::{db, max_step, peak, sine_fit};
use platter_testkit::signal::channel;

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;

/// Deck A at 1 kHz alone on the master, deck B at 300 Hz behind a closed crossfader, both
/// playing.
fn two_decks(name: &str) -> Rig {
    let mut rig = Rig::new();
    rig.load(A, sine_file(&format!("{name}-a"), 1_000.0, 10.0, 0.5, RATE));
    rig.load(B, sine_file(&format!("{name}-b"), 300.0, 10.0, 0.25, RATE));
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckPlay { deck: B });
    rig
}

/// Amplitude of the sine at `hz` in one output channel of a four-channel device, once the
/// decks have been playing for a while.
fn amplitude(rig: &mut Rig, output_channel: usize, hz: f64) -> f64 {
    rig.run_channels(0.2, 4);
    let output = rig.run_channels(0.5, 4);
    sine_fit(&channel(&output, 4, output_channel), hz, f64::from(RATE)).amplitude
}

#[test]
fn the_headphones_are_silent_until_a_deck_is_pre_listened() {
    let mut rig = two_decks("monitor-silent");
    let output = rig.run_channels(0.5, 4);
    assert!(channel(&output, 4, 2).iter().all(|sample| *sample == 0.0));
    assert!(channel(&output, 4, 3).iter().all(|sample| *sample == 0.0));
    assert!(peak(&channel(&output, 4, 0)) > 0.4);
}

#[test]
fn pre_listening_hears_a_deck_before_its_fader_and_the_crossfader() {
    let mut rig = two_decks("monitor-pfl");
    rig.send(Command::MixerFaderSet {
        deck: B,
        position: 0.0,
    });
    rig.send(Command::MixerCueToggle { deck: B });
    for side in [2, 3] {
        assert!((amplitude(&mut rig, side, 300.0) - 0.25).abs() < 0.003);
        // Deck A is not pre-listened and the mix knob is fully on the cue side.
        assert!(amplitude(&mut rig, side, 1_000.0) < 0.001);
    }
    // The master does not hear deck B at all.
    assert!(amplitude(&mut rig, 0, 300.0) < 0.001);
    // Switching it off fades the headphones out without a step.
    rig.send(Command::MixerCueToggle { deck: B });
    let closing = channel(&rig.run_channels(0.2, 4), 4, 2);
    assert!(max_step(&closing) < 0.03, "{}", max_step(&closing));
    assert!(closing[4_800..].iter().all(|sample| *sample == 0.0));
}

#[test]
fn the_mix_knob_blends_the_pre_listened_decks_with_the_master() {
    let mut rig = two_decks("monitor-mix");
    rig.send(Command::MixerCueToggle { deck: B });
    rig.send(Command::MixerCueMixSet { position: 0.5 });
    assert!((amplitude(&mut rig, 2, 300.0) - 0.125).abs() < 0.003);
    assert!((amplitude(&mut rig, 2, 1_000.0) - 0.25).abs() < 0.003);
    rig.send(Command::MixerCueMixSet { position: 1.0 });
    assert!(amplitude(&mut rig, 2, 300.0) < 0.001);
    assert!((amplitude(&mut rig, 2, 1_000.0) - 0.5).abs() < 0.003);
}

#[test]
fn the_headphone_volume_is_in_decibels_and_stays_inside_full_scale() {
    let mut rig = two_decks("monitor-volume");
    rig.send(Command::MixerCueToggle { deck: B });
    rig.send(Command::MixerCueVolumeSet { db: -6.0 });
    assert!((db(amplitude(&mut rig, 2, 300.0) / 0.25) + 6.0).abs() < 0.1);
    // Everything turned up: the headphones have their own limiter.
    rig.send(Command::MixerCueToggle { deck: A });
    rig.send(Command::MixerGainSet { deck: A, db: 12.0 });
    rig.send(Command::MixerGainSet { deck: B, db: 12.0 });
    rig.send(Command::MixerCueVolumeSet { db: 12.0 });
    let output = rig.run_channels(0.5, 4);
    assert!(
        output
            .iter()
            .all(|sample| sample.is_finite() && sample.abs() <= 1.0)
    );
    assert!(
        rig.engine
            .dispatch(Command::MixerCueVolumeSet { db: 12.5 })
            .is_err()
    );
}

#[test]
fn a_device_without_a_second_pair_gets_the_master_alone() {
    let mut rig = two_decks("monitor-stereo");
    rig.send(Command::MixerCueToggle { deck: B });
    rig.send(Command::MasterTestTone {
        output: Output::Headphones,
    });
    for channels in [2, 3] {
        rig.run_channels(0.2, channels);
        let output = rig.run_channels(0.5, channels);
        let left = channel(&output, channels, 0);
        assert!((sine_fit(&left, 1_000.0, f64::from(RATE)).amplitude - 0.5).abs() < 0.003);
        assert!(sine_fit(&left, 300.0, f64::from(RATE)).amplitude < 0.001);
        assert!(sine_fit(&left, 440.0, f64::from(RATE)).amplitude < 0.001);
    }
}

#[test]
fn the_test_tone_plays_for_a_second_on_the_output_it_is_sent_to() {
    for (output, sounding, quiet) in [(Output::Master, 0, 2), (Output::Headphones, 2, 0)] {
        let mut rig = Rig::new();
        rig.send(Command::MasterTestTone { output });
        let block = rig.run_channels(1.5, 4);
        let tone = channel(&block, 4, sounding);
        let fit = sine_fit(&tone[4_800..43_200], 440.0, f64::from(RATE));
        // 18 dB below full scale, on both sides of the pair.
        assert!((fit.amplitude - 0.125).abs() < 0.001, "{}", fit.amplitude);
        assert_eq!(tone, channel(&block, 4, sounding + 1));
        assert!(fit.snr_db() > 60.0, "{}", fit.snr_db());
        // It fades in and out, and it ends.
        assert!(max_step(&tone) < 0.01);
        assert!(tone[48_000 + 480..].iter().all(|sample| *sample == 0.0));
        assert!(
            channel(&block, 4, quiet)
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }
}
