//! The Input page of the preferences: the input device, what feeds each deck and the
//! auxiliary input, with a meter beside each.
//!
//! It follows the form pattern of the Audio page, as the design system asks for pages it does
//! not draw. Behaviour note: `docs/behavior/live-input.md`.

use egui::{Rect, Role, Sense, WidgetInfo};
use platter_core::DeckId;
use platter_engine::{Command, InputSource, InputTarget};

use super::{help, section, setting, setting_named};
use crate::command::{Action, AppCommand};
use crate::cx::{Cx, id};
use crate::model::{InputState, View};
use crate::paint;
use crate::panels::header::problem_keys;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::meter::{self, MeterMemory, MeterSpec};
use crate::widgets::value::{self, Steps, ValueSpec};

/// Inputs offered when the device does not say how many it has.
const DEFAULT_INPUTS: u16 = 2;
const ROW_STEP: f32 = 38.0;

/// The sources a device with `inputs` inputs can give: each input alone, then the pairs.
fn sources(inputs: u16) -> Vec<InputSource> {
    InputSource::ALL
        .into_iter()
        .filter(|source| u16::from(source.last_input()) < inputs)
        .collect()
}

pub(super) fn input_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let audio = view.audio;
    let mixer = &view.engine.mixer;

    let mut top = section(cx, page, 20.0, "settings.device");
    let control = setting(cx, page, top, "settings.input_device");
    let mut labels = vec![texts.get("settings.no_input")];
    labels.extend(audio.inputs.iter().map(|device| device.name.as_str()));
    let device_choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let current_device = match &view.settings.input_device {
        None => Some(0),
        Some(name) => audio
            .inputs
            .iter()
            .position(|device| &device.name == name)
            .map(|index| index + 1),
    };
    let select_rect = Rect::from_min_size(control.min, egui::vec2(280.0, tokens.size.button_h));
    let status = match audio.input {
        InputState::Off => texts.get("settings.input_off").to_owned(),
        InputState::Running { channels } => texts.format(
            "settings.input_running",
            &[("inputs", &channels.to_string())],
        ),
        // The title alone: the texts under it speak of the output.
        InputState::Stopped(problem) => texts.get(problem_keys(problem).0).to_owned(),
    };
    // The status runs under the whole row, as the help of the buffer size does.
    help(
        cx,
        Rect::from_min_max(
            select_rect.min,
            egui::pos2(control.right(), select_rect.bottom()),
        ),
        &status,
    );
    let line = Rect::from_min_size(
        egui::pos2(select_rect.left(), select_rect.bottom() + tokens.space.s2),
        egui::vec2(control.width(), 14.0),
    );
    let response = cx
        .ui
        .interact(line, id("settings-input-status"), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Status, true, &status));

    top = section(cx, page, top + 66.0, "settings.input_routing");
    let inputs = match audio.input {
        InputState::Running { channels } => channels,
        _ => DEFAULT_INPUTS,
    };
    let offered = sources(inputs);
    let mut source_labels = vec![texts.get("settings.input_none").to_owned()];
    source_labels.extend(
        offered
            .iter()
            .map(|source| texts.format("settings.input_source", &[("inputs", source.name())])),
    );
    let source_choices: Vec<Choice<'_>> = source_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let type_labels = [
        texts.get("settings.deck_track"),
        texts.get("settings.deck_live"),
    ];
    let type_choices: Vec<Choice<'_>> = type_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let levels = view.engine.input.levels;
    let mut selects = Vec::new();
    for target in InputTarget::ALL {
        let name = match target {
            InputTarget::Deck(deck) => texts.format(
                "settings.input_deck",
                &[("deck", &deck.letter().to_string())],
            ),
            InputTarget::Aux => texts.get("settings.input_aux").to_owned(),
            InputTarget::Recorder => texts.get("settings.input_recorder").to_owned(),
            InputTarget::FxSend => texts.get("settings.input_fx_send").to_owned(),
        };
        let control = setting_named(cx, page, top, &name);
        let route = mixer.input_routes[target.index()];
        let select = Rect::from_min_size(control.min, egui::vec2(136.0, tokens.size.button_h));
        let meter_rect = Rect::from_min_size(
            egui::pos2(select.right() + tokens.space.s4, control.center().y - 5.0),
            egui::vec2(96.0, 10.0),
        );
        meter::meter(
            cx,
            meter_rect,
            &MeterSpec {
                levels: route.map_or([0.0; 2], |source| source.take(&levels)),
                memory: MeterMemory::default(),
                segments: 16,
                vertical: false,
            },
        );
        let right = Rect::from_min_max(
            egui::pos2(meter_rect.right() + tokens.space.s5, control.top()),
            control.max,
        );
        match target {
            InputTarget::Deck(deck) => deck_type(cx, right, deck, view, &type_choices),
            InputTarget::Aux => aux_level(cx, right, mixer.aux_level),
            InputTarget::Recorder => recorder_source(cx, right, view),
            // The units that take it are chosen on the Mixer page.
            InputTarget::FxSend => {}
        }
        selects.push((select, target, name, route));
        top += ROW_STEP;
    }
    help(
        cx,
        Rect::from_min_max(
            egui::pos2(page.left() + super::CONTROL_X, page.top() + top - ROW_STEP),
            egui::pos2(
                page.right() - super::LABEL_X,
                page.top() + top - ROW_STEP + tokens.size.button_h,
            ),
        ),
        texts.get("settings.input_help"),
    );

    // The drop-downs last, so an open list lies over the controls below it.
    for (rect, target, name, route) in selects.into_iter().rev() {
        let current = match route {
            None => Some(0),
            Some(route) => offered
                .iter()
                .position(|source| *source == route)
                .map(|index| index + 1),
        };
        let offered = &offered;
        choice::select(
            cx,
            rect,
            id(("settings-input-route", target.index())),
            SelectSpec {
                name: &name,
                choices: &source_choices,
                current,
                enabled: true,
            },
            |index| {
                let source = index
                    .checked_sub(1)
                    .and_then(|index| offered.get(index))
                    .copied();
                Action::Engine(Command::InputRoutingSet { target, source })
            },
        );
    }
    let inputs = &audio.inputs;
    choice::select(
        cx,
        select_rect,
        id("settings-input-device"),
        SelectSpec {
            name: texts.get("settings.input_device"),
            choices: &device_choices,
            current: current_device,
            enabled: true,
        },
        |index| {
            let device = index.checked_sub(1).and_then(|index| inputs.get(index));
            AppCommand::InputDeviceSet {
                device: device.map(|device| device.name.clone()),
            }
            .into()
        },
    );
}

/// Track or live input, for one deck.
fn deck_type(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>, choices: &[Choice<'_>]) {
    let live = view
        .engine
        .decks
        .get(deck.index())
        .is_some_and(|snapshot| snapshot.controls.live);
    choice::segmented(
        cx,
        rect,
        id(("settings-deck-type", deck.index())),
        choices,
        Some(usize::from(live)),
        |index| {
            Action::Engine(Command::DeckTypeSet {
                deck,
                live: index == 1,
            })
        },
    );
}

/// The volume of the auxiliary input, as a number field in percent.
fn aux_level(cx: &mut Cx<'_>, rect: Rect, level: f64) {
    let theme = cx.theme;
    let texts = cx.texts;
    let field = Rect::from_min_size(rect.min, egui::vec2(72.0, theme.tokens.size.button_h));
    let text = format!("{:.0} %", level * 100.0);
    value::field(
        cx,
        field,
        id("settings-aux-level"),
        ValueSpec {
            name: texts.get("settings.aux_level"),
            position: level,
            default: 0.0,
            steps: Steps::PERCENT,
            enabled: true,
        },
        &text,
        |level| Action::Engine(Command::MixerAuxSet { level }),
    );
    paint::text_elided(
        cx.painter(),
        theme,
        Rect::from_min_max(
            egui::pos2(field.right() + theme.tokens.space.s3, rect.top()),
            rect.max,
        ),
        texts.get("settings.aux_level"),
        theme.tokens.text.body,
        theme.tokens.color.text_secondary,
    );
}

/// What the mix recorder records: the internal mix or this input.
fn recorder_source(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>) {
    let texts = cx.texts;
    let labels = [
        texts.get("settings.record_internal"),
        texts.get("settings.record_external"),
    ];
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    choice::segmented(
        cx,
        rect,
        id("settings-recorder-source"),
        &choices,
        Some(usize::from(view.engine.recorder.external)),
        |index| {
            Action::Engine(Command::RecorderSourceSet {
                external: index == 1,
            })
        },
    );
}
