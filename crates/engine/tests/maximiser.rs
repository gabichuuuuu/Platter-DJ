//! The Limiter key and the maximiser of the master: `docs/behavior/master.md`, *Maximiser*.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::{DeckId, SampleRate};
use platter_engine::{Command, CrossfaderCurve, Engine, MaximiserControls};
use platter_testkit::measure::{max_step, peak};
use platter_testkit::signal::channel;

use DeckId::{A, B};

/// The delay of the maximiser at 48 kHz: its window of 1.5 ms less one frame.
const DELAY: usize = 71;
/// -1 dB, the ceiling the maximiser starts with.
const CEILING: f32 = 0.891_250_9;

fn db(ratio: f32) -> f32 {
    20.0 * ratio.log10()
}

/// Two decks far above full scale: +12 dB of channel gain and of main volume. With
/// `limiter`, the key is switched on and its fade ends before the decks start.
fn overloaded(name: &str, limiter: bool) -> Rig {
    let mut rig = Rig::new();
    if limiter {
        rig.send(Command::MasterLimiterToggle);
        rig.run(0.05);
    }
    rig.load(A, sine_file(&format!("{name}-a"), 440.0, 4.0, 0.9, RATE));
    rig.load(B, sine_file(&format!("{name}-b"), 557.0, 4.0, 0.9, RATE));
    rig.send(Command::MixerCrossfaderCurveSet {
        curve: CrossfaderCurve::Sharp,
    });
    rig.send(Command::MixerGainSet { deck: A, db: 12.0 });
    rig.send(Command::MixerGainSet { deck: B, db: 12.0 });
    rig.send(Command::MasterGainSet { db: 12.0 });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckPlay { deck: B });
    rig
}

/// Deck A alone on the master, below the ceiling, pre-listened with the headphone mix in the
/// middle: the same deck reaches the headphones twice.
fn cued(name: &str, limiter: bool) -> Rig {
    let mut rig = Rig::with_deck_a(sine_file(name, 1_000.0, 2.0, 0.5, RATE));
    rig.send(Command::MixerCueToggle { deck: A });
    rig.send(Command::MixerCueMixSet { position: 0.5 });
    if limiter {
        rig.send(Command::MasterLimiterToggle);
    }
    rig.send(Command::DeckPlay { deck: A });
    rig
}

#[test]
fn the_limiter_key_starts_off_and_toggles_the_maximiser() {
    let mut rig = Rig::new();
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.mixer.maximiser, MaximiserControls::default());
    assert!(!snapshot.mixer.maximiser.on);
    rig.send(Command::MasterLimiterToggle);
    assert!(rig.engine.snapshot().mixer.maximiser.on);
    rig.send(Command::MasterLimiterToggle);
    assert!(!rig.engine.snapshot().mixer.maximiser.on);
}

/// `docs/behavior/master.md`: off, the master is the path of the safety limiter alone, so
/// a hot mix still reaches full scale.
#[test]
fn off_an_overloaded_mix_is_held_by_the_safety_limiter_alone() {
    let mut rig = overloaded("maximiser-off", false);
    let output = rig.run_stereo(1.0);
    assert!(peak(&output[48_000..]) > 0.95);
    assert_eq!(rig.engine.snapshot().maximiser_gain, 1.0);
}

#[test]
fn on_an_overloaded_mix_stays_under_the_ceiling_and_the_safety_limiter_rests() {
    let mut rig = overloaded("maximiser-on", true);
    let output = rig.run_stereo(1.0);
    assert!(output.iter().all(|sample| sample.is_finite()));
    let top = peak(&output);
    assert!(top <= CEILING, "{top}");
    assert!(db(CEILING) - db(top) < 0.5, "{top}");
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.limiter_gain, 1.0);
    assert!(snapshot.maximiser_gain < 0.1, "{}", snapshot.maximiser_gain);
    assert!(snapshot.master_levels.iter().all(|level| *level <= CEILING));
}

#[test]
fn the_ceiling_and_the_drive_set_the_level_of_the_master() {
    let mut rig = Rig::with_deck_a(sine_file("maximiser-ceiling", 1_000.0, 3.0, 0.5, RATE));
    rig.send(Command::MasterLimiterToggle);
    rig.send(Command::MasterMaximiserCeilingSet { db: -6.0 });
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.2);
    // 0.5 is 6.02 dB under full scale: the ceiling barely touches it.
    let quiet = peak(&rig.run(0.5));
    assert!((db(quiet) + 6.0).abs() < 0.05, "{}", db(quiet));
    // Twelve decibels of drive: the peaks stay at the ceiling.
    rig.send(Command::MasterMaximiserDriveSet { db: 12.0 });
    rig.run(0.2);
    let driven = peak(&rig.run(0.5));
    assert!((db(driven) + 6.0).abs() < 0.05, "{}", db(driven));
    let reduction = db(rig.engine.snapshot().maximiser_gain);
    assert!((reduction + 12.0).abs() < 0.2, "{reduction}");
}

/// Under the ceiling the maximiser only delays the master, by its window less a frame.
#[test]
fn under_the_ceiling_the_master_is_the_same_signal_later() {
    let mut off = Rig::with_deck_a(sine_file("maximiser-delay-off", 1_000.0, 2.0, 0.5, RATE));
    let mut on = Rig::with_deck_a(sine_file("maximiser-delay-on", 1_000.0, 2.0, 0.5, RATE));
    on.send(Command::MasterLimiterToggle);
    // Both rigs settle with the decks stopped: the fade of the key ends in silence.
    off.run(0.05);
    on.run(0.05);
    off.send(Command::DeckPlay { deck: A });
    on.send(Command::DeckPlay { deck: A });
    let dry = off.run(0.5);
    let delayed = on.run(0.5);
    assert_eq!(&delayed[DELAY..], &dry[..dry.len() - DELAY]);
}

/// `docs/behavior/master.md`: the headphones take the pre-listened decks as late as the
/// master, so a deck heard in both does not comb.
#[test]
fn the_headphones_stay_in_time_with_the_maximised_master() {
    let mut off = cued("maximiser-phones-off", false);
    let mut on = cued("maximiser-phones-on", true);
    let dry = off.run_channels(0.5, 4);
    let delayed = on.run_channels(0.5, 4);
    let phones = |output: &[f32]| channel(output, 4, 2);
    let (dry, delayed) = (phones(&dry), phones(&delayed));
    // The key fades in over the first 10 ms; afterwards the headphones are the same signal
    // later, so the mix of the deck with itself keeps its level.
    let from = 960;
    assert_eq!(&delayed[from + DELAY..], &dry[from..dry.len() - DELAY]);
    assert!(peak(&delayed[from..]) > 0.49, "{}", peak(&delayed[from..]));
}

/// Switching fades over 10 ms between the master as it is and the master 1.5 ms later: each
/// frame moves no more than the music does plus a 480th of the gap between the two, at most
/// twice the amplitude.
#[test]
fn switching_the_key_while_the_music_plays_does_not_click() {
    let mut rig = Rig::with_deck_a(sine_file("maximiser-switch", 200.0, 3.0, 0.8, RATE));
    rig.send(Command::DeckPlay { deck: A });
    let before = rig.run(0.2);
    let own = max_step(&before);
    let mut output = Vec::new();
    for _ in 0..3 {
        rig.send(Command::MasterLimiterToggle);
        output.extend(rig.run(0.2));
    }
    let step = max_step(&output);
    assert!(step <= own + 2.0 * 0.8 / 480.0, "{step} against {own}");
}

#[test]
fn values_outside_their_ranges_are_rejected_and_change_nothing() {
    let mut rig = Rig::new();
    for command in [
        Command::MasterMaximiserDriveSet { db: -0.1 },
        Command::MasterMaximiserDriveSet { db: 12.5 },
        Command::MasterMaximiserDriveSet { db: f64::NAN },
        Command::MasterMaximiserCeilingSet { db: 0.1 },
        Command::MasterMaximiserCeilingSet { db: -12.5 },
        Command::MasterMaximiserCeilingSet {
            db: f64::NEG_INFINITY,
        },
        Command::MasterMaximiserReleaseSet { ms: 9.0 },
        Command::MasterMaximiserReleaseSet { ms: 1_001.0 },
        Command::MasterMaximiserReleaseSet { ms: f64::INFINITY },
    ] {
        assert!(rig.engine.dispatch(command).is_err());
    }
    assert_eq!(
        rig.engine.snapshot().mixer.maximiser,
        MaximiserControls::default()
    );
    rig.send(Command::MasterMaximiserDriveSet { db: 3.0 });
    rig.send(Command::MasterMaximiserCeilingSet { db: -0.3 });
    rig.send(Command::MasterMaximiserReleaseSet { ms: 250.0 });
    assert_eq!(
        rig.engine.snapshot().mixer.maximiser,
        MaximiserControls {
            on: false,
            drive_db: 3.0,
            ceiling_db: -0.3,
            release_ms: 250.0,
        }
    );
}

/// A change of sample rate keeps the key and the settings, and the new processor applies
/// them.
#[test]
fn a_new_sample_rate_keeps_the_maximiser() {
    let mut rig = Rig::new();
    rig.send(Command::MasterLimiterToggle);
    rig.send(Command::MasterMaximiserCeilingSet { db: -3.0 });
    rig.send(Command::MasterMaximiserReleaseSet { ms: 400.0 });
    let rate = SampleRate::new(44_100).unwrap();
    let (mut engine, mut processor) = Engine::with_controls_of(rate, &rig.engine);
    assert_eq!(
        engine.snapshot().mixer.maximiser,
        rig.engine.snapshot().mixer.maximiser
    );
    let path = sine_file("maximiser-rate", 1_000.0, 2.0, 0.9, 44_100);
    engine
        .dispatch(Command::MixerCrossfaderSet { position: 0.0 })
        .expect("crossfader");
    engine
        .dispatch(Command::DeckLoad { deck: A, path })
        .expect("load");
    engine.wait_for_loads();
    engine
        .dispatch(Command::DeckPlay { deck: A })
        .expect("play");
    let mut output = vec![0.0f32; 44_100 * 2];
    for block in output.chunks_mut(512) {
        processor.process(block, 2);
    }
    // -3 dB is 0.708: a tone at 0.9 comes out at the ceiling.
    let top = peak(&output[44_100..]);
    assert!((db(top) + 3.0).abs() < 0.05, "{}", db(top));
}
