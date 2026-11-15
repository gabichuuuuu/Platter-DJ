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

/// Lines that look like a binding, right or wrong, to reach the checks behind the syntax.
fn line() -> impl Strategy<Value = String> {
    let key = prop::sample::select(vec![
        "midi", "command", "control", "hold", "release", "when", "param", "range", "invert",
        "step", "encoding", "modifier", "value", "deck",
    ]);
    let value = prop::sample::select(vec![
        r#""note 1 36""#,
        r#""cc 16 119""#,
        r#""pitch 3""#,
        r#""button""#,
        r#""fader""#,
        r#""encoder""#,
        r#""3f-41""#,
        "true",
        "-1",
        "8",
        "1e999",
        "nan",
        "[0, 1]",
        r#"["1 = 1", "8 = 7"]"#,
        r#"{ action = "release" }"#,
        r#""x""#,
    ]);
    (key, value).prop_map(|(key, value)| format!("{key} = {value}"))
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
        bindings in prop::collection::vec(prop::collection::vec(line(), 0..8), 0..6),
        messages in prop::collection::vec(prop::collection::vec(any::<u8>(), 3), 0..16),
    ) {
        let source: String = bindings
            .iter()
            .map(|lines| format!("[[binding]]\n{}\n", lines.join("\n")))
            .collect();
        if let Ok(mapping) = Mapping::parse(&source) {
            let _ = mapping.commands();
            let mut mapper = Mapper::new(mapping);
            for bytes in &messages {
                let _ = mapper.handle(bytes);
            }
        }
    }
}
