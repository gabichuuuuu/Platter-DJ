//! HID bindings of a mapping and the reports that drive them: `docs/behavior/hid-controllers.md`.
//! Every report is synthesised.

#![allow(clippy::unwrap_used, clippy::panic)]

use platter_control::{Arg, BindingProblem, Emit, MAX_REPORT_BYTES, Mapper, Mapping, MappingError};

fn mapper(source: &str) -> Mapper {
    Mapper::new(Mapping::parse(source).unwrap())
}

fn problem(source: &str) -> BindingProblem {
    match Mapping::parse(source) {
        Err(MappingError::Binding { problem, .. }) => problem,
        other => panic!("{other:?}"),
    }
}

fn commands(emits: &[Emit]) -> Vec<&str> {
    emits.iter().map(|emit| emit.command.as_str()).collect()
}

fn number(emit: &Emit, param: &str) -> f64 {
    match emit.args.get(param) {
        Some(Arg::Number(value)) => *value,
        other => panic!("{param}: {other:?}"),
    }
}

const PLAY: &str = r#"
[[binding]]
hid = "bit 1 0 3"
command = "deck.play_pause"
deck = "a"
"#;

#[test]
fn a_bit_presses_when_it_is_set_and_releases_when_it_clears() {
    let mut mapper = mapper(&format!(
        "{PLAY}hold = true\nrelease = {{ deck = \"a\" }}\n"
    ));
    // Report 1, bit 3 of its first byte.
    let pressed = mapper.handle_report(&[1, 0b1000]);
    assert_eq!(commands(&pressed), ["deck.play_pause"]);
    // The same report again changes nothing.
    assert!(mapper.handle_report(&[1, 0b1000]).is_empty());
    // Another bit of the byte changes; this one does not.
    assert!(mapper.handle_report(&[1, 0b1001]).is_empty());
    assert_eq!(
        commands(&mapper.handle_report(&[1, 0b0001])),
        ["deck.play_pause"]
    );
}

#[test]
fn a_report_of_another_id_does_nothing_for_a_binding() {
    let mut mapper = mapper(PLAY);
    assert!(mapper.handle_report(&[2, 0xFF]).is_empty());
    assert_eq!(
        commands(&mapper.handle_report(&[1, 0xFF])),
        ["deck.play_pause"]
    );
}

#[test]
fn report_zero_counts_from_the_first_byte_of_a_device_without_ids() {
    let mut mapper = mapper(
        r#"
[[binding]]
hid = "bit 0 1 0"
command = "deck.cue"
deck = "b"
"#,
    );
    assert!(mapper.handle_report(&[0xFF, 0]).is_empty());
    assert_eq!(commands(&mapper.handle_report(&[0xFF, 1])), ["deck.cue"]);
}

#[test]
fn a_byte_and_a_word_are_faders_over_their_range() {
    let mut mapper = mapper(
        r#"
[[binding]]
hid = "byte 1 0"
command = "mixer.crossfader.set"
param = "position"

[[binding]]
hid = "word 1 1"
command = "mixer.fader.set"
deck = "a"
param = "position"
range = [0, 2]
"#,
    );
    let emits = mapper.handle_report(&[1, 255, 0x00, 0x80]);
    assert_eq!(
        commands(&emits),
        ["mixer.crossfader.set", "mixer.fader.set"]
    );
    assert_eq!(number(&emits[0], "position"), 1.0);
    // Little-endian: 0x8000 is a little more than half of 65 535.
    assert!((number(&emits[1], "position") - 2.0 * 32_768.0 / 65_535.0).abs() < 1e-12);
    // Only the word moves.
    let emits = mapper.handle_report(&[1, 255, 0xFF, 0xFF]);
    assert_eq!(commands(&emits), ["mixer.fader.set"]);
    assert_eq!(number(&emits[0], "position"), 2.0);
}

#[test]
fn a_counter_steps_by_its_difference_and_goes_round() {
    let mut mapper = mapper(
        r#"
[[binding]]
hid = "byte 1 2"
control = "encoder"
encoding = "counter"
command = "browser.move"
param = "delta"
"#,
    );
    // The first report is where it starts.
    assert!(mapper.handle_report(&[1, 0, 0, 250]).is_empty());
    let emits = mapper.handle_report(&[1, 0, 0, 3]);
    assert_eq!(number(&emits[0], "delta"), 9.0);
    let emits = mapper.handle_report(&[1, 0, 0, 1]);
    assert_eq!(number(&emits[0], "delta"), -2.0);
    assert!(mapper.handle_report(&[1, 0, 0, 1]).is_empty());
}

#[test]
fn a_relative_byte_steps_on_every_report_that_carries_steps() {
    let mut mapper = mapper(
        r#"
[[binding]]
hid = "byte 1 0"
control = "encoder"
command = "browser.move"
param = "delta"
"#,
    );
    // 1 is one step up, the same value again is another one; 255 is one step down.
    for (byte, delta) in [(1, 1.0), (1, 1.0), (255, -1.0), (129, -127.0)] {
        let emits = mapper.handle_report(&[1, byte]);
        assert_eq!(number(&emits[0], "delta"), delta, "{byte}");
    }
    assert!(mapper.handle_report(&[1, 0]).is_empty());
    assert!(mapper.handle_report(&[1, 128]).is_empty());
}

#[test]
fn a_report_too_short_for_a_binding_leaves_it_as_it_was() {
    let mut mapper = mapper(
        r#"
[[binding]]
hid = "byte 1 4"
command = "mixer.crossfader.set"
param = "position"
"#,
    );
    assert!(mapper.handle_report(&[1, 0, 0]).is_empty());
    assert!(mapper.handle_report(&[]).is_empty());
    assert_eq!(mapper.handle_report(&[1, 0, 0, 0, 0, 128]).len(), 1);
}

#[test]
fn midi_and_hid_bindings_live_in_one_mapping() {
    let mut mapper = mapper(&format!(
        "{PLAY}\n[[binding]]\nmidi = \"note 1 36\"\ncommand = \"deck.cue\"\ndeck = \"a\"\n"
    ));
    assert_eq!(commands(&mapper.handle(&[0x90, 36, 100])), ["deck.cue"]);
    assert_eq!(
        commands(&mapper.handle_report(&[1, 0b1000])),
        ["deck.play_pause"]
    );
}

#[test]
fn bindings_that_do_not_fit_their_part_of_the_report_are_refused() {
    let binding = |rest: &str| format!("[[binding]]\ncommand = \"deck.cue\"\n{rest}\n");
    for (rest, expected) in [
        (
            "hid = \"bit 1 0 8\"",
            BindingProblem::BadHid("bit 1 0 8".to_owned()),
        ),
        (
            "hid = \"byte 256 0\"",
            BindingProblem::BadHid("byte 256 0".to_owned()),
        ),
        (
            &format!("hid = \"word 0 {}\"", MAX_REPORT_BYTES - 1) as &str,
            BindingProblem::BadHid(format!("word 0 {}", MAX_REPORT_BYTES - 1)),
        ),
        (
            "hid = \"bit 1 0 0\"\ncontrol = \"fader\"\nparam = \"position\"",
            BindingProblem::ControlDoesNotFit,
        ),
        (
            "hid = \"word 1 0\"\ncontrol = \"encoder\"\nparam = \"delta\"",
            BindingProblem::ControlDoesNotFit,
        ),
        (
            "midi = \"note 1 36\"\nhid = \"bit 1 0 0\"",
            BindingProblem::NotForThisControl("hid".to_owned()),
        ),
        (
            "midi = \"cc 1 7\"\ncontrol = \"encoder\"\nencoding = \"counter\"\nparam = \"delta\"",
            BindingProblem::BadEncoding("counter".to_owned()),
        ),
    ] {
        assert_eq!(problem(&binding(rest)), expected, "{rest}");
    }
}

#[test]
fn a_report_longer_than_the_limit_is_read_up_to_it() {
    let mut mapper = mapper(&format!(
        "[[binding]]\nhid = \"byte 0 {}\"\ncommand = \"mixer.crossfader.set\"\nparam = \"position\"\n",
        MAX_REPORT_BYTES - 1
    ));
    let mut report = vec![0; MAX_REPORT_BYTES + 10];
    report[MAX_REPORT_BYTES - 1] = 255;
    assert_eq!(mapper.handle_report(&report).len(), 1);
}
