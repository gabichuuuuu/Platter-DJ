//! The mixer: channel strips, crossfader and master section.
//!
//! Behaviour notes: `docs/behavior/mixer.md` and `docs/behavior/master.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::{Band, Command, CrossfaderCurve, Engine, FADER_UNITY, TEMPO_RANGES};
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
    // A squared taper around unity at three quarters of the travel: (0.5 / 0.75) squared.
    assert!((settled_amplitude(&mut rig, 100.0) - 0.5 * 4.0 / 9.0).abs() < 0.002);
    rig.send(Command::MixerFaderSet {
        deck: A,
        position: 0.0,
    });
    let closing = rig.run(0.1);
    // 10 ms of ramp, and no step on the way.
    assert!(closing[960..].iter().all(|sample| *sample == 0.0));
    assert!(max_step(&closing) < 0.01);
    // The meter falls at its own pace after the signal is gone: 20 dB in 300 ms.
    let level = rig.engine.snapshot().decks[0].levels[0];
    assert!(level > 0.05 && level < 0.15, "{level}");
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
    assert!((rig.engine.snapshot().master_levels[0] - amplitude as f32).abs() < 0.005);
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
    assert!(snapshot.master_levels.iter().all(|level| *level <= 1.0));
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

#[test]
fn the_fader_is_at_unity_by_default_and_adds_gain_above_it() {
    let mut rig = playing_deck_a("mixer-fader-unity", 1_000.0);
    assert_eq!(rig.engine.snapshot().decks[0].controls.fader, FADER_UNITY);
    rig.send(Command::MixerFaderSet {
        deck: A,
        position: FADER_UNITY,
    });
    assert!((settled_amplitude(&mut rig, 1_000.0) - 0.5).abs() < 0.003);
    rig.send(Command::MixerFaderSet {
        deck: A,
        position: 1.0,
    });
    // The top of the travel is (1 / 0.75) squared: 5 dB above unity.
    let amplitude = settled_amplitude(&mut rig, 1_000.0);
    assert!((db(amplitude / 0.5) - 5.0).abs() < 0.1, "{amplitude}");
}

#[test]
fn a_kill_button_removes_its_band_and_gives_it_back() {
    let mut rig = playing_deck_a("mixer-kill", 60.0);
    let kill = Command::MixerEqKillToggle {
        deck: A,
        band: Band::Low,
    };
    rig.send(kill.clone());
    assert!(rig.engine.snapshot().decks[0].controls.eq_kill[0]);
    let cut = rig.run(0.5);
    // The band is gone, without a step on the way out.
    assert!(peak(&cut[24_000..]) < 0.01, "{}", peak(&cut[24_000..]));
    assert!(max_step(&cut) < 0.01);
    // The knob keeps its position while the band is cut, and takes over again afterwards.
    rig.send(Command::MixerEqSet {
        deck: A,
        band: Band::Low,
        position: 0.5,
    });
    assert!(peak(&rig.run(0.2)[4_800..]) < 0.01);
    rig.send(kill);
    assert!(!rig.engine.snapshot().decks[0].controls.eq_kill[0]);
    assert!((settled_amplitude(&mut rig, 60.0) - 0.5).abs() < 0.01);
}

#[test]
fn pan_fades_the_far_side_and_leaves_the_near_side_alone() {
    let mut rig = playing_deck_a("mixer-pan", 1_000.0);
    let sides = |rig: &mut Rig| {
        rig.run(0.2);
        let output = rig.run_stereo(0.5);
        [0, 1].map(|side| sine_fit(&channel(&output, 2, side), 1_000.0, f64::from(RATE)).amplitude)
    };
    let [left, right] = sides(&mut rig);
    assert!((left - 0.5).abs() < 0.003 && (right - 0.5).abs() < 0.003);
    rig.send(Command::MixerPanSet {
        deck: A,
        position: -0.5,
    });
    let [left, right] = sides(&mut rig);
    assert!(
        (left - 0.5).abs() < 0.003 && (right - 0.25).abs() < 0.003,
        "{left} {right}"
    );
    rig.send(Command::MixerPanSet {
        deck: A,
        position: 1.0,
    });
    // No step beyond what the sine itself does between two samples (0.065).
    let moving = rig.run_stereo(0.1);
    assert!(max_step(&channel(&moving, 2, 0)) < 0.07);
    let [left, right] = sides(&mut rig);
    assert!(left < 1e-6 && (right - 0.5).abs() < 0.003, "{left} {right}");
    let levels = rig.engine.snapshot().decks[0].levels;
    assert!(
        levels[0] < 0.05 && (levels[1] - 0.5).abs() < 0.01,
        "{levels:?}"
    );
}

#[test]
fn the_tempo_range_cycles_and_pulls_the_tempo_inside() {
    let mut rig = playing_deck_a("mixer-range", 440.0);
    let controls = |rig: &Rig| rig.engine.snapshot().decks[0].controls.clone();
    assert_eq!(controls(&rig).tempo_range, TEMPO_RANGES[0]);
    rig.send(Command::DeckTempoRangeCycle { deck: A });
    rig.send(Command::DeckTempoRangeCycle { deck: A });
    assert_eq!(controls(&rig).tempo_range, 0.5);
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.4,
    });
    assert_eq!(controls(&rig).tempo_ratio, 1.4);
    // Back to the narrowest range: the speed moves to the edge the fader can reach.
    rig.send(Command::DeckTempoRangeCycle { deck: A });
    assert_eq!(controls(&rig).tempo_range, 0.08);
    assert_eq!(controls(&rig).tempo_ratio, 1.08);
    rig.run(0.2);
    let fit = sine_fit(&rig.run(0.5), 440.0 * 1.08, f64::from(RATE));
    assert!((fit.amplitude - 0.5).abs() < 0.005, "{}", fit.amplitude);
}

#[test]
fn the_snapshot_carries_where_every_control_stands() {
    let mut rig = Rig::new();
    rig.send(Command::MixerGainSet { deck: B, db: -4.5 });
    rig.send(Command::MixerEqSet {
        deck: B,
        band: Band::High,
        position: 0.8,
    });
    rig.send(Command::MixerFilterSet {
        deck: B,
        position: -0.3,
    });
    rig.send(Command::MixerPanSet {
        deck: B,
        position: 0.25,
    });
    rig.send(Command::MixerFaderSet {
        deck: B,
        position: 0.6,
    });
    rig.send(Command::MixerCueToggle { deck: B });
    rig.send(Command::MixerCrossfaderSet { position: 0.9 });
    rig.send(Command::MixerCrossfaderCurveSet {
        curve: CrossfaderCurve::Sharp,
    });
    rig.send(Command::MixerCueMixSet { position: 0.7 });
    rig.send(Command::MixerCueVolumeSet { db: -9.0 });
    rig.send(Command::MasterGainSet { db: 3.0 });
    // A refused command leaves the control where it was.
    assert!(
        rig.engine
            .dispatch(Command::MixerPanSet {
                deck: B,
                position: 4.0
            })
            .is_err()
    );
    let snapshot = rig.engine.snapshot();
    let deck = &snapshot.decks[1].controls;
    assert_eq!(deck.gain_db, -4.5);
    assert_eq!(deck.eq, [0.5, 0.5, 0.8]);
    assert_eq!((deck.filter, deck.pan, deck.fader), (-0.3, 0.25, 0.6));
    assert!(deck.pre_listen && !snapshot.decks[0].controls.pre_listen);
    assert_eq!(snapshot.mixer.crossfader, 0.9);
    assert_eq!(snapshot.mixer.crossfader_curve, CrossfaderCurve::Sharp);
    assert_eq!(snapshot.mixer.headphone_mix, 0.7);
    assert_eq!(snapshot.mixer.headphone_gain_db, -9.0);
    assert_eq!(snapshot.mixer.master_gain_db, 3.0);
}

#[test]
fn an_engine_at_another_rate_keeps_the_controls() {
    let mut rig = Rig::new();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::MixerGainSet { deck: A, db: -6.0 });
    rig.send(Command::MixerEqKillToggle {
        deck: A,
        band: Band::High,
    });
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.05,
    });
    let before = rig.engine.snapshot();
    let rate = platter_core::SampleRate::new(44_100).unwrap();
    let (mut engine, mut processor) = Engine::with_controls_of(rate, &rig.engine);
    let after = engine.snapshot();
    assert_eq!(after.mixer, before.mixer);
    assert_eq!(after.decks[0].controls, before.decks[0].controls);
    assert!(after.decks[0].track.is_none());

    // The processor stands there too: the same track sounds 6 dB down and 5 % faster.
    engine
        .dispatch(Command::DeckLoad {
            deck: A,
            path: sine_file("mixer-carry", 200.0, 4.0, 0.5, 44_100),
        })
        .unwrap();
    engine.wait_for_loads();
    engine.dispatch(Command::DeckPlay { deck: A }).unwrap();
    let mut output = vec![0.0f32; 44_100 * 2];
    for block in output.chunks_mut(512) {
        processor.process(block, 2);
    }
    let left = channel(&output, 2, 0);
    let fit = sine_fit(&left[22_050..], 210.0, 44_100.0);
    assert!(
        (db(fit.amplitude / 0.5) + 6.0).abs() < 0.1,
        "{}",
        fit.amplitude
    );
}
