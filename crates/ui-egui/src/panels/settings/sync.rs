//! The Sync page of the preferences: where the MIDI clock goes, whether it is sent, and its
//! offset.
//!
//! It follows the form pattern of the Audio page, as the design system asks for pages it does
//! not draw. Behaviour note: `docs/behavior/midi-clock.md`.

use egui::Rect;
use platter_engine::Command;

use super::{Switch, help, section, setting, switch};
use crate::command::{Action, AppCommand};
use crate::cx::{Cx, id};
use crate::model::View;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::key::{self, KeySpec, Press};
use crate::widgets::value::{self, Steps, ValueSpec};

/// The offset runs from this many milliseconds early to as many late.
const OFFSET_RANGE_MS: f64 = 50.0;

pub(super) fn sync_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let settings = view.settings;
    let clock = &view.engine.clock;

    let top = section(cx, page, 20.0, "settings.midi_clock");
    let control = setting(cx, page, top, "settings.clock_output");
    let mut labels = vec![texts.get("settings.no_output")];
    labels.extend(settings.clock_outputs.iter().map(String::as_str));
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let current = match &settings.clock_output {
        None => Some(0),
        Some(name) => settings
            .clock_outputs
            .iter()
            .position(|port| port == name)
            .map(|index| index + 1),
    };
    let select_rect = Rect::from_min_size(control.min, egui::vec2(280.0, tokens.size.button_h));
    let status = match (&settings.clock_output, settings.clock_output_open) {
        (None, _) => texts.get("settings.clock_nowhere"),
        (Some(_), true) => texts.get("settings.clock_ready"),
        (Some(_), false) => texts.get("settings.clock_closed"),
    };
    help(
        cx,
        Rect::from_min_max(
            select_rect.min,
            egui::pos2(control.right(), select_rect.bottom()),
        ),
        status,
    );

    switch(
        cx,
        page,
        top + 56.0,
        Switch {
            name: "settings.clock_send",
            help: "settings.clock_send_help",
            on: clock.midi_clock,
            action: Command::ClockMidiSendToggle.into(),
        },
    );

    let control = setting(cx, page, top + 106.0, "settings.clock_restart");
    let label = texts.get("settings.restart");
    let width = key::width_for(cx, label, 0.0);
    key::key(
        cx,
        Rect::from_min_size(control.min, egui::vec2(width, tokens.size.button_h)),
        id("settings-clock-restart"),
        // Always enabled: a restart asked while no clock goes out does nothing, and the key
        // stays where the keyboard finds it.
        KeySpec::text(label),
        Press::Click(Command::ClockMidiRestart.into()),
    );

    let control = setting(cx, page, top + 146.0, "settings.clock_offset");
    let field = Rect::from_min_size(control.min, egui::vec2(88.0, tokens.size.button_h));
    let ms = f64::from(settings.clock_offset_ms);
    value::field(
        cx,
        field,
        id("settings-clock-offset"),
        ValueSpec {
            name: texts.get("settings.clock_offset"),
            position: (ms + OFFSET_RANGE_MS) / (2.0 * OFFSET_RANGE_MS),
            default: 0.5,
            steps: Steps::PERCENT,
            enabled: true,
        },
        &texts.format("settings.offset_value", &[("ms", &format!("{ms:+.0}"))]),
        |position| {
            AppCommand::ClockMidiOffsetSet {
                ms: (position * 2.0 * OFFSET_RANGE_MS - OFFSET_RANGE_MS).round() as i32,
            }
            .into()
        },
    );
    help(
        cx,
        Rect::from_min_max(field.min, egui::pos2(control.right(), field.bottom())),
        texts.get("settings.clock_offset_help"),
    );

    // Before the list of outputs, so an open list of outputs lies over this section.
    follow_section(cx, page, top + 230.0, view);

    let outputs = &settings.clock_outputs;
    choice::select(
        cx,
        select_rect,
        id("settings-clock-output"),
        SelectSpec {
            name: texts.get("settings.clock_output"),
            choices: &choices,
            current,
            enabled: true,
        },
        |index| {
            let port = index.checked_sub(1).and_then(|index| outputs.get(index));
            Action::App(AppCommand::ClockMidiOutputSet {
                port: port.cloned(),
            })
        },
    );
}

/// The clock received: the input, the source of the master clock and what arrives.
fn follow_section(cx: &mut Cx<'_>, page: Rect, top: f32, view: &View<'_>) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let settings = view.settings;
    let clock = &view.engine.clock;

    let top = section(cx, page, top, "settings.follow_clock");
    let control = setting(cx, page, top, "settings.clock_input");
    let select_rect = Rect::from_min_size(control.min, egui::vec2(280.0, tokens.size.button_h));
    let status = match (
        &settings.clock_input,
        settings.clock_input_open,
        settings.clock_received,
    ) {
        (None, _, _) => texts.get("settings.clock_no_input").to_owned(),
        (Some(_), false, _) => texts.get("settings.clock_input_closed").to_owned(),
        (Some(_), true, None) => texts.get("settings.clock_waiting").to_owned(),
        (Some(_), true, Some(tenths)) => texts.format(
            "settings.clock_receiving",
            &[("bpm", &format!("{:.1}", f64::from(tenths) / 10.0))],
        ),
    };
    help(
        cx,
        Rect::from_min_max(
            select_rect.min,
            egui::pos2(control.right(), select_rect.bottom()),
        ),
        &status,
    );

    let control = setting(cx, page, top + 56.0, "settings.clock_source");
    let source_labels = [
        texts.get("settings.source_internal"),
        texts.get("settings.source_external"),
    ];
    let source_choices: Vec<Choice<'_>> = source_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-clock-source"),
        &source_choices,
        Some(usize::from(clock.external)),
        |index| {
            Action::Engine(Command::ClockSourceSet {
                external: index == 1,
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.clock_source_help"),
    );

    let mut labels = vec![texts.get("settings.no_input")];
    labels.extend(settings.clock_inputs.iter().map(String::as_str));
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let current = match &settings.clock_input {
        None => Some(0),
        Some(name) => settings
            .clock_inputs
            .iter()
            .position(|port| port == name)
            .map(|index| index + 1),
    };
    let inputs = &settings.clock_inputs;
    choice::select(
        cx,
        select_rect,
        id("settings-clock-input"),
        SelectSpec {
            name: texts.get("settings.clock_input"),
            choices: &choices,
            current,
            enabled: true,
        },
        |index| {
            let port = index.checked_sub(1).and_then(|index| inputs.get(index));
            Action::App(AppCommand::ClockMidiInputSet {
                port: port.cloned(),
            })
        },
    );
}
