//! Property test: no sequence of commands, valid or not, makes the engine panic, produce a
//! value that is not a number or leave full scale.

mod common;

use std::path::PathBuf;
use std::sync::OnceLock;

use common::{RATE, Rig, sine_file};
use platter_core::DeckId;
use platter_engine::command::{Args, SPECS, Value};
use platter_engine::{Band, Command, CrossfaderCurve};
use proptest::prelude::{ProptestConfig, Strategy, any, prop, prop_oneof, proptest};

/// Three files to load: two tracks and one that does not exist.
fn files() -> &'static [PathBuf; 3] {
    static FILES: OnceLock<[PathBuf; 3]> = OnceLock::new();
    FILES.get_or_init(|| {
        [
            sine_file("sweep-a", 440.0, 0.4, 0.9, RATE),
            sine_file("sweep-b", 97.0, 0.3, 0.9, 44_100),
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sweep-missing.wav"),
        ]
    })
}

/// Numbers of every kind: inside the ranges, around their edges, huge, and not numbers.
fn number() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => -1.5f64..2.5,
        2 => -100.0f64..30.0,
        1 => any::<f64>(),
        1 => prop::sample::select(vec![
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -0.0,
            1.0,
            f64::MAX,
            f64::MIN_POSITIVE,
        ]),
    ]
}

fn deck() -> impl Strategy<Value = DeckId> {
    prop::sample::select(DeckId::ALL.to_vec())
}

fn command() -> impl Strategy<Value = Command> {
    let band = prop::sample::select(vec![Band::Low, Band::Mid, Band::High]);
    let curve = prop::sample::select(CrossfaderCurve::ALL.to_vec());
    prop_oneof![
        (deck(), 0usize..3).prop_map(|(deck, file)| Command::DeckLoad {
            deck,
            path: files()[file].clone()
        }),
        deck().prop_map(|deck| Command::DeckEject { deck }),
        deck().prop_map(|deck| Command::DeckPlay { deck }),
        deck().prop_map(|deck| Command::DeckPause { deck }),
        deck().prop_map(|deck| Command::DeckPlayPause { deck }),
        deck().prop_map(|deck| Command::DeckCueSet { deck }),
        deck().prop_map(|deck| Command::DeckCueGoto { deck }),
        (deck(), number()).prop_map(|(deck, seconds)| Command::DeckSeek { deck, seconds }),
        (deck(), number()).prop_map(|(deck, ratio)| Command::DeckTempoSet { deck, ratio }),
        (deck(), number()).prop_map(|(deck, amount)| Command::DeckNudge { deck, amount }),
        (deck(), number()).prop_map(|(deck, db)| Command::MixerGainSet { deck, db }),
        (deck(), band, number()).prop_map(|(deck, band, position)| Command::MixerEqSet {
            deck,
            band,
            position
        }),
        (deck(), number()).prop_map(|(deck, position)| Command::MixerFilterSet { deck, position }),
        (deck(), number()).prop_map(|(deck, position)| Command::MixerFaderSet { deck, position }),
        number().prop_map(|position| Command::MixerCrossfaderSet { position }),
        curve.prop_map(|curve| Command::MixerCrossfaderCurveSet { curve }),
        number().prop_map(|db| Command::MasterGainSet { db }),
    ]
}

fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        number().prop_map(Value::Number),
        "[a-z./ ]{0,8}".prop_map(Value::Text),
        prop::sample::select(vec!["a", "b", "low", "high", "sharp"])
            .prop_map(|text| Value::Text(text.to_owned())),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn no_command_sequence_breaks_the_output(
        steps in prop::collection::vec((command(), 0usize..1_500, 1usize..5), 1..40),
    ) {
        let mut rig = Rig::new();
        let mut block = Vec::new();
        for (command, frames, channels) in steps {
            // Invalid commands are refused; that is an outcome, not a failure.
            let _ = rig.engine.dispatch(command);
            rig.engine.wait_for_loads();
            block.clear();
            block.resize(frames * channels, f32::NAN);
            rig.processor.process(&mut block, channels);
            for sample in &block {
                assert!(sample.is_finite() && sample.abs() <= 1.0, "{sample}");
            }
        }
        let snapshot = rig.engine.snapshot();
        for deck in &snapshot.decks {
            assert!(deck.position_seconds.is_finite() && deck.position_seconds >= 0.0);
            assert!(deck.cue_seconds.is_finite() && deck.level.is_finite());
        }
        assert!(snapshot.master_level.is_finite() && snapshot.master_level <= 1.0);
    }

    #[test]
    fn untyped_input_builds_a_command_or_an_error(
        spec in prop::sample::select(SPECS.to_vec()),
        use_real_id in any::<bool>(),
        other_id in "[a-z._]{0,24}",
        values in prop::collection::vec(value(), 0..5),
        extra in prop::option::of(("[a-z]{1,6}", value())),
    ) {
        let mut args: Args = spec
            .params
            .iter()
            .zip(values)
            .map(|(param, value)| (param.name.to_owned(), value))
            .collect();
        if let Some((name, value)) = extra {
            args.insert(name, value);
        }
        let id = if use_real_id { spec.id } else { other_id.as_str() };
        if let Ok(command) = Command::from_args(id, &args) {
            assert_eq!(command.id(), id);
            let mut rig = Rig::new();
            let _ = rig.engine.dispatch(command);
            rig.engine.wait_for_loads();
        }
    }
}
