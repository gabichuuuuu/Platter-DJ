//! The mixer: channel strips, crossfader and master section.
//!
//! Behaviour notes: `docs/behavior/mixer.md` and `docs/behavior/master.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Band, Command, CrossfaderCurve};
use platter_testkit::measure::{db, max_step, peak, rms, sine_fit};
use platter_testkit::signal::channel;

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;

/// Amplitude of the sine at `hz` in the output once the deck has been playing for a while.
fn settled_amplitude(rig: &mut Rig, hz: f64) -> f64 {
    rig.run(0.2);
    sine_fit(&rig.run(0.5), hz, f64::from(RATE)).amplitude
}

fn playing_deck_a(name: &str, hz: f64) -> Rig {
    let mut rig = Rig::with_deck_a(sine_file(name, hz, 10.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig
}

#[test]
fn the_default_mixer_passes_the_track_at_unity() {
    let mut rig = playing_deck_a("mixer-unity", 1_000.0);
    assert!((settled_amplitude(&mut rig, 1_000.0) - 0.5).abs() < 0.003);
}

#[test]
fn channel_gain_is_in_decibels() {
    let mut rig = playing_deck_a("mixer-gain", 1_000.0);
    rig.send(Command::MixerGainSet { deck: A, db: -6.0 });
    let amplitude = settled_amplitude(&mut rig, 1_000.0);
    assert!((db(amplitude / 0.5) + 6.0).abs() < 0.1);
    rig.send(Command::MixerGainSet {
        deck: A,
        db: f64::NEG_INFINITY,
    });
    rig.run(0.1);
    // What is left is the tail of the equaliser filters, far below anything audible.
    assert!(peak(&rig.run(0.1)) < 1e-12);
}

#[test]
fn the_fader_closes_to_silence() {
    let mut rig = playing_deck_a("mixer-fader", 100.0);
    rig.send(Command::MixerFaderSet {
        deck: A,
        position: 0.5,
    });
    assert!((settled_amplitude(&mut rig, 100.0) - 0.125).abs() < 0.002);
    rig.send(Command::MixerFaderSet {
        deck: A,
        position: 0.0,
    });
    let closing = rig.run(0.1);
    // 10 ms of ramp, and no step on the way.
    assert!(closing[960..].iter().all(|sample| *sample == 0.0));
    assert!(max_step(&closing) < 0.01);
    // The meter falls at its own pace after the signal is gone.
    assert!(rig.engine.snapshot().decks[0].level < 0.1);
}

#[test]
fn killing_an_equaliser_band_removes_it() {
    let mut rig = playing_deck_a("mixer-eq-low", 60.0);
    rig.send(Command::MixerEqSet {
        deck: A,
        band: Band::Low,
        position: 0.0,
    });
    assert!(db(settled_amplitude(&mut rig, 60.0) / 0.5) < -45.0);

    let mut rig = playing_deck_a("mixer-eq-high", 8_000.0);
    rig.send(Command::MixerEqSet {
        deck: A,
        band: Band::Low,
        position: 0.0,
    });
    assert!(db(settled_amplitude(&mut rig, 8_000.0) / 0.5).abs() < 0.1);
    rig.send(Command::MixerEqSet {
        deck: A,
        band: Band::High,
        position: 0.0,
    });
    assert!(db(settled_amplitude(&mut rig, 8_000.0) / 0.5) < -35.0);
}

#[test]
fn the_filter_is_a_low_pass_one_way_and_a_high_pass_the_other() {
    let mut rig = playing_deck_a("mixer-filter-high-tone", 8_000.0);
    rig.send(Command::MixerFilterSet {
        deck: A,
        position: -0.6,
    });
    assert!(db(settled_amplitude(&mut rig, 8_000.0) / 0.5) < -30.0);
    rig.send(Command::MixerFilterSet {
        deck: A,
        position: 0.6,
    });
    assert!(db(settled_amplitude(&mut rig, 8_000.0) / 0.5).abs() < 1.0);
    rig.send(Command::MixerFilterSet {
        deck: A,
        position: 0.0,
    });
    assert!((settled_amplitude(&mut rig, 8_000.0) - 0.5).abs() < 0.003);
}

#[test]
fn the_crossfader_blends_the_decks_by_its_curve() {
    let mut rig = Rig::new();
    rig.load(A, sine_file("mixer-xf-a", 440.0, 10.0, 0.5, RATE));
    rig.load(B, sine_file("mixer-xf-b", 1_000.0, 10.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckPlay { deck: B });
    let amplitudes = |rig: &mut Rig| {
        rig.run(0.2);
        let output = rig.run(0.5);
        (
            sine_fit(&output, 440.0, f64::from(RATE)).amplitude,
            sine_fit(&output, 1_000.0, f64::from(RATE)).amplitude,
        )
    };

    // Centre of the default curve: each side 3 dB down.
    let (a, b) = amplitudes(&mut rig);
    assert!(
        (a - 0.3536).abs() < 0.003 && (b - 0.3536).abs() < 0.003,
        "{a} {b}"
    );

    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    let (a, b) = amplitudes(&mut rig);
    assert!((a - 0.5).abs() < 0.003 && b < 1e-4, "{a} {b}");

    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    let (a, b) = amplitudes(&mut rig);
    assert!(a < 1e-4 && (b - 0.5).abs() < 0.003, "{a} {b}");

    rig.send(Command::MixerCrossfaderSet { position: 0.5 });
    rig.send(Command::MixerCrossfaderCurveSet {
        curve: CrossfaderCurve::Linear,
    });
    let (a, b) = amplitudes(&mut rig);
    assert!(
        (a - 0.25).abs() < 0.003 && (b - 0.25).abs() < 0.003,
        "{a} {b}"
    );

    rig.send(Command::MixerCrossfaderCurveSet {
        curve: CrossfaderCurve::Sharp,
    });
    let (a, b) = amplitudes(&mut rig);
    assert!(
        (a - 0.5).abs() < 0.003 && (b - 0.5).abs() < 0.003,
        "{a} {b}"
    );
}

#[test]
fn moving_the_crossfader_or_changing_its_curve_does_not_step() {
    let mut rig = playing_deck_a("mixer-xf-step", 100.0);
    let mut output = rig.run(0.2);
    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    output.extend(rig.run(0.1));
    rig.send(Command::MixerCrossfaderSet { position: 0.5 });
    rig.send(Command::MixerCrossfaderCurveSet {
        curve: CrossfaderCurve::Sharp,
    });
    output.extend(rig.run(0.1));
    assert!(max_step(&output) < 0.01, "{}", max_step(&output));
}

#[test]
fn main_volume_is_in_decibels() {
    let mut rig = playing_deck_a("master-gain", 1_000.0);
    rig.send(Command::MasterGainSet { db: -12.0 });
    let amplitude = settled_amplitude(&mut rig, 1_000.0);
    assert!((db(amplitude / 0.5) + 12.0).abs() < 0.1);
    assert!((rig.engine.snapshot().master_level - amplitude as f32).abs() < 0.005);
    assert_eq!(rig.engine.snapshot().limiter_gain, 1.0);
}

#[test]
fn the_limiter_keeps_an_overloaded_mix_inside_full_scale() {
    let mut rig = Rig::new();
    rig.load(A, sine_file("master-limit-a", 440.0, 10.0, 0.9, RATE));
    rig.load(B, sine_file("master-limit-b", 557.0, 10.0, 0.9, RATE));
    rig.send(Command::MixerCrossfaderCurveSet {
        curve: CrossfaderCurve::Sharp,
    });
    rig.send(Command::MixerGainSet { deck: A, db: 12.0 });
    rig.send(Command::MixerGainSet { deck: B, db: 12.0 });
    rig.send(Command::MasterGainSet { db: 12.0 });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckPlay { deck: B });
    let output = rig.run_stereo(1.0);
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(peak(&output) <= 1.0);
    // Turned down rather than squared off: a clipped mix this hot would sit near 1.0 RMS.
    let level = rms(&output[48_000..]);
    assert!(level < 0.8, "{level}");
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.limiter_gain < 0.1, "{}", snapshot.limiter_gain);
    assert!(snapshot.master_level <= 1.0);
}

#[test]
fn the_output_layout_follows_the_channel_count() {
    let mut rig = playing_deck_a("mixer-layout", 440.0);
    rig.run(0.1);

    let mut mono = vec![9.0; 480];
    rig.processor.process(&mut mono, 1);
    assert!(peak(&mono) > 0.4 && peak(&mono) <= 0.5);

    let mut four = vec![9.0; 480 * 4];
    rig.processor.process(&mut four, 4);
    assert!(peak(&channel(&four, 4, 0)) > 0.4);
    assert_eq!(channel(&four, 4, 0), channel(&four, 4, 1));
    assert_eq!(peak(&channel(&four, 4, 2)), 0.0);
    assert_eq!(peak(&channel(&four, 4, 3)), 0.0);

    // A buffer that is not a whole number of frames has its tail silenced.
    let mut ragged = vec![9.0; 7];
    rig.processor.process(&mut ragged, 2);
    assert_eq!(ragged[6], 0.0);
    let mut none = vec![9.0; 8];
    rig.processor.process(&mut none, 0);
    assert_eq!(none, vec![0.0; 8]);
}
