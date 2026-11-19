//! The models of the equaliser and of the filter, chosen for every channel at once.
//!
//! Behaviour note: `docs/behavior/mixer.md`, *Equaliser and filter models*.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::{DeckId, SampleRate};
use platter_engine::command::{Args, Value};
use platter_engine::{Band, Command, Engine, EqModel, FilterModel};
use platter_testkit::measure::{db, max_step, sine_fit};
use platter_testkit::signal::channel;

const A: DeckId = DeckId::A;

fn playing_deck_a(name: &str, hz: f64) -> Rig {
    let mut rig = Rig::with_deck_a(sine_file(name, hz, 10.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig
}

/// Level in decibels of the sine at `hz`, relative to the 0.5 of the file, once the ramps
/// and the filters have settled.
fn level_db(rig: &mut Rig, hz: f64) -> f64 {
    rig.run(0.3);
    db(sine_fit(&rig.run(0.5), hz, f64::from(RATE)).amplitude / 0.5)
}

#[test]
fn the_equaliser_model_sets_how_deep_a_removed_band_cuts_outside_it() {
    // Two octaves below the edge between the low and the mid band.
    let mut rig = playing_deck_a("models-eq", 62.5);
    rig.send(Command::MixerEqKillToggle {
        deck: A,
        band: Band::Low,
    });
    assert_eq!(rig.engine.snapshot().mixer.eq_model, EqModel::Isolator);
    let mut levels = Vec::new();
    for model in EqModel::ALL {
        rig.send(Command::MixerEqModelSet { model });
        assert_eq!(rig.engine.snapshot().mixer.eq_model, model);
        levels.push(level_db(&mut rig, 62.5));
    }
    // 6, 12, 24 and 48 dB per octave.
    let expected = [-12.3, -24.6, -48.2];
    for (level, expected) in levels.iter().zip(expected) {
        assert!((level - expected).abs() < 1.0, "{levels:?}");
    }
    assert!(levels[3] < -80.0, "{levels:?}");
}

#[test]
fn the_ladder_filter_lifts_the_corner_the_others_leave_level() {
    // Half way to the left the corner of the low-pass sits near 1.1 kHz.
    let mut rig = playing_deck_a("models-filter", 1_000.0);
    rig.send(Command::MixerFilterSet {
        deck: A,
        position: -0.5,
    });
    let mut levels = Vec::new();
    for model in FilterModel::ALL {
        rig.send(Command::MixerFilterModelSet { model });
        assert_eq!(rig.engine.snapshot().mixer.filter_model, model);
        levels.push(level_db(&mut rig, 1_000.0));
    }
    let [gentle, steep, ladder] = levels[..] else {
        panic!("{levels:?}");
    };
    assert!(gentle.abs() < 1.5 && steep < 0.0, "{levels:?}");
    assert!(ladder > gentle + 4.0, "{levels:?}");
}

#[test]
fn changing_a_model_while_the_deck_plays_does_not_step() {
    let mut rig = playing_deck_a("models-switch", 180.0);
    rig.send(Command::MixerFilterSet {
        deck: A,
        position: -0.3,
    });
    rig.run(0.2);
    let steady = max_step(&rig.run(0.1));
    let mut output = Vec::new();
    for (eq, filter) in EqModel::ALL
        .into_iter()
        .zip(FilterModel::ALL.into_iter().cycle())
    {
        rig.send(Command::MixerEqModelSet { model: eq });
        rig.send(Command::MixerFilterModelSet { model: filter });
        output.extend(rig.run(0.05));
    }
    assert!(output.iter().all(|sample| sample.is_finite()));
    assert!(
        max_step(&output) < 1.5 * steady,
        "{} against {steady}",
        max_step(&output)
    );
}

#[test]
fn a_model_the_mixer_does_not_have_is_rejected() {
    let mut args = Args::new();
    args.insert("model".to_owned(), Value::Text("analogue".to_owned()));
    assert!(Command::from_args("mixer.eq.model.set", &args).is_err());
    assert!(Command::from_args("mixer.filter.model.set", &args).is_err());
    args.insert("model".to_owned(), Value::Text("ladder".to_owned()));
    assert_eq!(
        Command::from_args("mixer.filter.model.set", &args),
        Ok(Command::MixerFilterModelSet {
            model: FilterModel::Ladder
        })
    );
    args.insert("model".to_owned(), Value::Text("steep".to_owned()));
    assert_eq!(
        Command::from_args("mixer.eq.model.set", &args),
        Ok(Command::MixerEqModelSet {
            model: EqModel::Steep
        })
    );
}

#[test]
fn an_engine_at_another_rate_keeps_the_models() {
    let mut rig = Rig::new();
    rig.send(Command::MixerEqModelSet {
        model: EqModel::Smooth,
    });
    rig.send(Command::MixerFilterModelSet {
        model: FilterModel::Ladder,
    });
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::MixerEqKillToggle {
        deck: A,
        band: Band::Low,
    });
    let rate = SampleRate::new(44_100).unwrap();
    let (mut engine, mut processor) = Engine::with_controls_of(rate, &rig.engine);
    let mixer = engine.snapshot().mixer;
    assert_eq!(
        (mixer.eq_model, mixer.filter_model),
        (EqModel::Smooth, FilterModel::Ladder)
    );
    // The new processor has the smooth model: a removed low band still lets a tone two
    // octaves below its edge through at about -12 dB.
    engine
        .dispatch(Command::DeckLoad {
            deck: A,
            path: sine_file("models-carry", 62.5, 4.0, 0.5, 44_100),
        })
        .unwrap();
    engine.wait_for_loads();
    engine.dispatch(Command::DeckPlay { deck: A }).unwrap();
    let mut output = vec![0.0f32; 44_100 * 2];
    for block in output.chunks_mut(512) {
        processor.process(block, 2);
    }
    let left = channel(&output, 2, 0);
    let level = db(sine_fit(&left[22_050..], 62.5, 44_100.0).amplitude / 0.5);
    assert!((level + 12.3).abs() < 1.0, "{level}");
}
