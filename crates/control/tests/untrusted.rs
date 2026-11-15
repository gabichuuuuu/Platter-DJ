//! Mapping files and MIDI bytes are untrusted input: random ones must never panic.
//! Behaviour note: `docs/behavior/controllers.md`, section *Limits*.

#![allow(clippy::unwrap_used)]

use platter_control::{Arg, Mapper, Mapping};
use proptest::prelude::*;

const MAPPING: &str = r#"
[[binding]]
midi = "note 1 40"
modifier = 1
value = 1
hold = true

[[binding]]
midi = "note 1 36"
command = "deck.cue"
action = "press"
hold = true
release = { action = "release" }
when = ["1 = 1"]

[[binding]]
midi = "cc 1 7"
command = "mixer.fader.set"
param = "position"
range = [-1e308, 1e308]

[[binding]]
midi = "cc 1 30"
control = "encoder"
command = "deck.nudge"
param = "amount"
step = 1e308

[[binding]]
midi = "pitch 1"
command = "mixer.filter.set"
param = "position"
invert = true
"#;

/// A binding with a valid control and command, and other keys, each at most once, with
/// values right or wrong, so that many cases reach the checks behind the syntax and the mapper.
fn binding() -> impl Strategy<Value = String> {
    let control = prop::sample::select(vec![
        r#""note 1 36""#,
        r#""cc 1 7""#,
        r#""cc 1 119""#,
        r#""pitch 1""#,
    ]);
    let keys = prop::sample::subsequence(
        vec![
            "command", "control", "hold", "release", "when", "param", "range", "invert", "step",
            "encoding", "up", "down", "modifier", "value", "deck",
        ],
        0..=6,
    );
    let value = prop::sample::select(vec![
        r#""button""#,
        r#""fader""#,
        r#""encoder""#,
        r#""3f-41""#,
        r#""position""#,
        "true",
        "-1",
        "1",
        "8",
        "1e999",
        "nan",
        "[0, 1]",
        "[-1e308, 1e308]",
        r#"["1 = 1", "8 = 7"]"#,
        r#"{ action = "release" }"#,
        r#""x""#,
    ]);
    (control, keys)
        .prop_flat_map(move |(control, keys)| {
            let count = keys.len();
            (
                Just(control),
                Just(keys),
                prop::collection::vec(value.clone(), count),
            )
        })
        .prop_map(|(control, keys, values)| {
            let mut lines = vec![format!("midi = {control}")];
            lines.extend(
                keys.iter()
                    .zip(values)
                    .map(|(key, value)| format!("{key} = {value}")),
            );
            format!("[[binding]]\n{}\n", lines.join("\n"))
        })
}

/// Messages on the controls the bindings above use.
fn message() -> impl Strategy<Value = Vec<u8>> {
    (
        prop::sample::select(vec![0x80u8, 0x90, 0xB0, 0xE0]),
        prop::sample::select(vec![36u8, 7, 119, 0, 127]),
        any::<u8>(),
    )
        .prop_map(|(status, first, second)| vec![status, first, second & 0x7F])
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn random_bytes_never_panic(messages in prop::collection::vec(
        prop::collection::vec(any::<u8>(), 0..5), 0..64)
    ) {
        let mut mapper = Mapper::new(Mapping::parse(MAPPING).unwrap());
        for bytes in &messages {
            for emit in mapper.handle(bytes) {
                prop_assert!(!emit.command.is_empty());
                for arg in emit.args.values() {
                    if let Arg::Number(number) = arg {
                        prop_assert!(number.is_finite(), "{emit:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn random_text_never_panics(source in "\\PC*") {
        let _ = Mapping::parse(&source);
    }

    #[test]
    fn random_bindings_never_panic(
        bindings in prop::collection::vec(binding(), 1..6),
        messages in prop::collection::vec(message(), 0..32),
    ) {
        let source: String = bindings.concat();
        if let Ok(mapping) = Mapping::parse(&source) {
            let _ = mapping.commands();
            let mut mapper = Mapper::new(mapping);
            for bytes in &messages {
                for emit in mapper.handle(bytes) {
                    for arg in emit.args.values() {
                        if let Arg::Number(number) = arg {
                            prop_assert!(number.is_finite(), "{emit:?}");
                        }
                    }
                }
            }
        }
    }
}
