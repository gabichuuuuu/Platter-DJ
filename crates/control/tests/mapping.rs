//! Mappings and the commands their messages trigger, on invented mappings.
//! Behaviour note: `docs/behavior/controllers.md`.

#![allow(clippy::unwrap_used, clippy::panic)]

use platter_control::{Arg, BindingProblem, Emit, Mapper, Mapping, MappingError};

fn mapper(source: &str) -> Mapper {
    Mapper::new(Mapping::parse(source).unwrap())
}

fn emit(command: &str, args: &[(&str, Arg)]) -> Emit {
    Emit {
        command: command.to_owned(),
        args: args
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    }
}

fn text(value: &str) -> Arg {
    Arg::Text(value.to_owned())
}

fn problem(source: &str) -> BindingProblem {
    match Mapping::parse(source) {
        Err(MappingError::Binding { number: 1, problem }) => problem,
        other => panic!("{other:?}"),
    }
}

const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;
const CC: u8 = 0xB0;

#[test]
fn a_button_sends_its_command_on_press_only() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "note 1 36"
        command = "deck.play_pause"
        deck = "a"
        "#,
    );
    let play = emit("deck.play_pause", &[("deck", text("a"))]);
    assert_eq!(
        mapper.handle(&[NOTE_ON, 36, 100]),
        std::slice::from_ref(&play)
    );
    // A second press without a release, and the release, do nothing.
    assert!(mapper.handle(&[NOTE_ON, 36, 90]).is_empty());
    assert!(mapper.handle(&[NOTE_ON, 36, 0]).is_empty());
    assert_eq!(mapper.handle(&[NOTE_ON, 36, 1]), [play]);
    assert!(mapper.handle(&[NOTE_OFF, 36, 0]).is_empty());
    // Another note, another channel.
    assert!(mapper.handle(&[NOTE_ON, 37, 100]).is_empty());
    assert!(mapper.handle(&[NOTE_ON | 1, 36, 100]).is_empty());
}

#[test]
fn a_button_on_hold_sends_its_release_with_the_parameters_it_names() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "cc 16 20"
        control = "button"
        command = "deck.cue"
        deck = "b"
        action = "press"
        hold = true
        release = { action = "release" }
        "#,
    );
    assert_eq!(
        mapper.handle(&[CC | 15, 20, 127]),
        [emit(
            "deck.cue",
            &[("deck", text("b")), ("action", text("press"))]
        )]
    );
    assert_eq!(
        mapper.handle(&[CC | 15, 20, 0]),
        [emit(
            "deck.cue",
            &[("deck", text("b")), ("action", text("release"))]
        )]
    );
}

const SHIFTED: &str = r#"
[[binding]]
midi = "note 1 40"
modifier = 1
value = 1
hold = true

[[binding]]
midi = "note 1 36"
command = "deck.play_pause"
deck = "a"
when = ["1 = 0"]

[[binding]]
midi = "note 1 36"
command = "deck.cue"
deck = "a"
action = "press"
hold = true
release = { action = "release" }
when = ["1 = 1"]
"#;

#[test]
fn modifiers_choose_which_binding_acts() {
    let mut mapper = mapper(SHIFTED);
    let play = emit("deck.play_pause", &[("deck", text("a"))]);
    assert_eq!(mapper.handle(&[NOTE_ON, 36, 100]), [play]);
    mapper.handle(&[NOTE_OFF, 36, 0]);
    assert!(mapper.handle(&[NOTE_ON, 40, 100]).is_empty());
    assert_eq!(mapper.modifiers()[0], 1);
    let cue = |action| emit("deck.cue", &[("deck", text("a")), ("action", text(action))]);
    assert_eq!(mapper.handle(&[NOTE_ON, 36, 100]), [cue("press")]);
    // The modifier goes back to 0, and the button still sends the release of what it did.
    mapper.handle(&[NOTE_OFF, 40, 0]);
    assert_eq!(mapper.modifiers()[0], 0);
    assert_eq!(mapper.handle(&[NOTE_OFF, 36, 0]), [cue("release")]);
}

#[test]
fn a_modifier_without_hold_keeps_its_value_and_two_conditions_must_both_hold() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "note 1 1"
        modifier = 2
        value = 7

        [[binding]]
        midi = "note 1 2"
        modifier = 8
        value = 3

        [[binding]]
        midi = "note 1 3"
        command = "global.snap.toggle"
        when = ["2 = 7", "8=3"]
        "#,
    );
    mapper.handle(&[NOTE_ON, 1, 100]);
    mapper.handle(&[NOTE_OFF, 1, 0]);
    assert!(mapper.handle(&[NOTE_ON, 3, 100]).is_empty());
    mapper.handle(&[NOTE_OFF, 3, 0]);
    mapper.handle(&[NOTE_ON, 2, 100]);
    assert_eq!(mapper.modifiers(), [0, 7, 0, 0, 0, 0, 0, 3]);
    assert_eq!(
        mapper.handle(&[NOTE_ON, 3, 100]),
        [emit("global.snap.toggle", &[])]
    );
}

#[test]
fn a_fader_sends_its_position_in_its_range() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "cc 1 7"
        command = "mixer.fader.set"
        deck = "a"
        param = "position"

        [[binding]]
        midi = "cc 1 8"
        command = "mixer.gain.set"
        deck = "a"
        param = "db"
        range = [-24, 12]
        invert = true

        [[binding]]
        midi = "pitch 2"
        command = "mixer.filter.set"
        deck = "b"
        param = "position"
        range = [-1.0, 1.0]
        "#,
    );
    let position = |mapper: &mut Mapper, bytes: &[u8], param: &str| {
        let emits = mapper.handle(bytes);
        assert_eq!(emits.len(), 1);
        match emits[0].args[param] {
            Arg::Number(value) => value,
            Arg::Text(_) => panic!(),
        }
    };
    assert_eq!(position(&mut mapper, &[CC, 7, 0], "position"), 0.0);
    assert_eq!(position(&mut mapper, &[CC, 7, 127], "position"), 1.0);
    assert_eq!(position(&mut mapper, &[CC, 8, 0], "db"), 12.0);
    assert_eq!(position(&mut mapper, &[CC, 8, 127], "db"), -24.0);
    // The pitch bend: the bottom, the centre and the top.
    let bend = 0xE1;
    assert_eq!(position(&mut mapper, &[bend, 0, 0], "position"), -1.0);
    assert_eq!(position(&mut mapper, &[bend, 0x00, 0x40], "position"), 0.0);
    assert_eq!(position(&mut mapper, &[bend, 0x7F, 0x7F], "position"), 1.0);
    let emits = mapper.handle(&[CC, 7, 64]);
    assert_eq!(emits[0].command, "mixer.fader.set");
    assert_eq!(emits[0].args["deck"], text("a"));
}

#[test]
fn an_encoder_sends_its_steps_in_both_encodings() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "cc 1 30"
        control = "encoder"
        command = "browser.move"
        param = "delta"
        step = 0.5

        [[binding]]
        midi = "cc 1 31"
        control = "encoder"
        command = "browser.move"
        param = "delta"
        encoding = "3f-41"
        "#,
    );
    let amount = |mapper: &mut Mapper, number: u8, value: u8| {
        mapper
            .handle(&[CC, number, value])
            .first()
            .map(|emit| emit.args["delta"].clone())
    };
    let n = Arg::Number;
    assert_eq!(amount(&mut mapper, 30, 1), Some(n(0.5)));
    assert_eq!(amount(&mut mapper, 30, 3), Some(n(1.5)));
    assert_eq!(amount(&mut mapper, 30, 127), Some(n(-0.5)));
    assert_eq!(amount(&mut mapper, 30, 65), Some(n(-31.5)));
    assert_eq!(amount(&mut mapper, 30, 0), None);
    assert_eq!(amount(&mut mapper, 30, 64), None);
    assert_eq!(amount(&mut mapper, 31, 65), Some(n(1.0)));
    assert_eq!(amount(&mut mapper, 31, 63), Some(n(-1.0)));
    assert_eq!(amount(&mut mapper, 31, 0), None);
    assert_eq!(amount(&mut mapper, 31, 64), None);
}

#[test]
fn every_binding_on_a_control_acts_in_the_order_of_the_file() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "note 3 0"
        command = "deck.play_pause"
        deck = "a"

        [[binding]]
        midi = "note 3 0"
        command = "deck.play_pause"
        deck = "b"
        "#,
    );
    let emits = mapper.handle(&[NOTE_ON | 2, 0, 1]);
    assert_eq!(
        emits,
        [
            emit("deck.play_pause", &[("deck", text("a"))]),
            emit("deck.play_pause", &[("deck", text("b"))]),
        ]
    );
}

#[test]
fn the_commands_of_a_mapping_can_be_checked_when_it_is_read() {
    let mapping = Mapping::parse(SHIFTED).unwrap();
    assert_eq!(mapping.len(), 3);
    let commands = mapping.commands();
    assert_eq!(
        commands,
        [
            emit("deck.play_pause", &[("deck", text("a"))]),
            emit(
                "deck.cue",
                &[("deck", text("a")), ("action", text("press"))]
            ),
            emit(
                "deck.cue",
                &[("deck", text("a")), ("action", text("release"))]
            ),
        ]
    );
}

#[test]
fn mistakes_name_the_binding_and_what_is_wrong() {
    let cases: [(&str, BindingProblem); 16] = [
        ("x = 1", BindingProblem::MissingMidi),
        (
            r#"midi = "note 0 1""#,
            BindingProblem::BadMidi("note 0 1".to_owned()),
        ),
        (
            r#"midi = "note 17 1""#,
            BindingProblem::BadMidi("note 17 1".to_owned()),
        ),
        (
            r#"midi = "cc 1 120""#,
            BindingProblem::BadMidi("cc 1 120".to_owned()),
        ),
        (
            r#"midi = "note 1 128""#,
            BindingProblem::BadMidi("note 1 128".to_owned()),
        ),
        (r#"midi = "note 1 1""#, BindingProblem::MissingCommand),
        (
            r#"midi = "note 1 1"
            control = "fader"
            command = "x""#,
            BindingProblem::ControlDoesNotFit,
        ),
        (
            r#"midi = "pitch 1"
            control = "encoder"
            command = "x""#,
            BindingProblem::ControlDoesNotFit,
        ),
        (
            r#"midi = "cc 1 1"
            control = "knob"
            command = "x""#,
            BindingProblem::BadControl("knob".to_owned()),
        ),
        (
            r#"midi = "note 1 1"
            modifier = 9
            value = 0"#,
            BindingProblem::BadModifier,
        ),
        (
            r#"midi = "note 1 1"
            command = "x"
            when = ["1 = 8"]"#,
            BindingProblem::BadCondition("1 = 8".to_owned()),
        ),
        (
            r#"midi = "note 1 1"
            command = "x"
            when = ["1 = 0", "2 = 0", "3 = 0"]"#,
            BindingProblem::TooManyConditions,
        ),
        (
            r#"midi = "cc 1 1"
            command = "x""#,
            BindingProblem::MissingParam,
        ),
        (
            r#"midi = "cc 1 1"
            command = "x"
            param = "position"
            range = [0, "top"]"#,
            BindingProblem::BadRange,
        ),
        (
            r#"midi = "note 1 1"
            command = "x"
            hold = true"#,
            BindingProblem::MissingRelease,
        ),
        (
            r#"midi = "note 1 1"
            command = "x"
            param = "position""#,
            BindingProblem::NotForThisControl("param".to_owned()),
        ),
    ];
    for (body, expected) in cases {
        let source = format!("[[binding]]\n{body}\n");
        assert_eq!(problem(&source), expected, "{body}");
    }
    let second = "[[binding]]\nmidi = \"note 1 1\"\ncommand = \"x\"\n\n[[binding]]\nmidi = \"x\"\n";
    assert!(matches!(
        Mapping::parse(second),
        Err(MappingError::Binding { number: 2, .. })
    ));
    assert!(matches!(
        Mapping::parse("binding = 3"),
        Err(MappingError::Syntax(_))
    ));
    assert!(Mapping::parse("").unwrap().is_empty());
}

#[test]
fn files_too_large_or_with_too_many_bindings_are_refused() {
    let large = format!("# {}\n", "x".repeat(platter_control::MAX_FILE_BYTES));
    assert_eq!(Mapping::parse(&large), Err(MappingError::TooLarge));
    let many = "[[binding]]\nmidi = \"note 1 1\"\ncommand = \"x\"\n"
        .repeat(platter_control::MAX_BINDINGS + 1);
    assert_eq!(Mapping::parse(&many), Err(MappingError::TooManyBindings));
}

#[test]
fn messages_of_other_kinds_are_ignored() {
    let mut mapper = mapper(SHIFTED);
    for bytes in [
        &[][..],
        &[NOTE_ON, 36],
        &[NOTE_ON, 36, 0x80],
        &[0xC0, 36, 0],
        &[0xF8],
        &[36, 100, 0],
    ] {
        assert!(mapper.handle(bytes).is_empty(), "{bytes:?}");
    }
}

#[test]
fn a_value_too_large_for_a_number_is_not_sent() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "cc 1 30"
        control = "encoder"
        command = "browser.move"
        param = "delta"
        step = 1e308

        [[binding]]
        midi = "cc 1 7"
        command = "mixer.fader.set"
        param = "position"
        range = [-1e308, 1e308]
        "#,
    );
    assert_eq!(mapper.handle(&[CC, 30, 1]).len(), 1);
    assert!(mapper.handle(&[CC, 30, 3]).is_empty());
    assert!(mapper.handle(&[CC, 7, 64]).is_empty());
}

#[test]
fn an_encoder_can_send_one_command_per_step_each_way() {
    let mut mapper = mapper(
        r#"
        [[binding]]
        midi = "cc 1 40"
        control = "encoder"
        command = "deck.beatjump"
        deck = "a"
        up = { direction = "forward" }
        down = { direction = "back" }
        "#,
    );
    let jump = |direction| {
        emit(
            "deck.beatjump",
            &[("deck", text("a")), ("direction", text(direction))],
        )
    };
    assert_eq!(
        mapper.handle(&[CC, 40, 2]),
        [jump("forward"), jump("forward")]
    );
    assert_eq!(mapper.handle(&[CC, 40, 127]), [jump("back")]);
    // A fast turn sends at most a bounded number of commands.
    assert_eq!(
        mapper.handle(&[CC, 40, 63]).len(),
        platter_control::MAX_REPEATS
    );
    assert!(mapper.handle(&[CC, 40, 0]).is_empty());
}

#[test]
fn the_commands_checked_cover_both_ends_of_faders_and_encoders() {
    let mapping = Mapping::parse(
        r#"
        [[binding]]
        midi = "cc 1 8"
        command = "mixer.gain.set"
        deck = "a"
        param = "db"
        range = [-24, 12]

        [[binding]]
        midi = "cc 1 9"
        control = "encoder"
        command = "browser.move"
        param = "delta"
        step = 2

        [[binding]]
        midi = "cc 1 40"
        control = "encoder"
        command = "deck.beatjump"
        deck = "b"
        up = { direction = "forward" }
        down = { direction = "back" }
        "#,
    )
    .unwrap();
    let n = Arg::Number;
    assert_eq!(
        mapping.commands(),
        [
            emit("mixer.gain.set", &[("deck", text("a")), ("db", n(-24.0))]),
            emit("mixer.gain.set", &[("deck", text("a")), ("db", n(12.0))]),
            emit("browser.move", &[("delta", n(2.0))]),
            emit("browser.move", &[("delta", n(-2.0))]),
            emit(
                "deck.beatjump",
                &[("deck", text("b")), ("direction", text("forward"))]
            ),
            emit(
                "deck.beatjump",
                &[("deck", text("b")), ("direction", text("back"))]
            ),
        ]
    );
}

#[test]
fn an_encoder_mixing_its_two_ways_and_a_command_with_a_modifier_are_refused() {
    for (body, expected) in [
        (
            "midi = \"cc 1 1\"\ncontrol = \"encoder\"\ncommand = \"x\"\nparam = \"delta\"\nup = { a = 1 }\ndown = { a = 2 }",
            BindingProblem::MixedSteps,
        ),
        (
            "midi = \"cc 1 1\"\ncontrol = \"encoder\"\ncommand = \"x\"\nup = { a = 1 }",
            BindingProblem::MixedSteps,
        ),
        (
            "midi = \"cc 1 1\"\ncontrol = \"encoder\"\ncommand = \"x\"\nstep = 2\nup = { a = 1 }\ndown = { a = 2 }",
            BindingProblem::MixedSteps,
        ),
        (
            "midi = \"note 1 1\"\ncommand = \"x\"\nmodifier = 1\nvalue = 1",
            BindingProblem::NotForThisControl("modifier".to_owned()),
        ),
    ] {
        assert_eq!(
            problem(&format!("[[binding]]\n{body}\n")),
            expected,
            "{body}"
        );
    }
}
