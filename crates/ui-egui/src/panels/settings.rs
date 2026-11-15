//! The preferences dialog: the Audio, General and Mixer pages.
//!
//! Component: `Select` in `docs/design/DESIGN.md`; placement from
//! `docs/design/screens/Preferences-audio.html` and `Preferences-general.html`; the Mixer
//! page follows the form pattern of the Audio page, as the design system asks. Behaviour
//! note: `docs/behavior/settings.md`.

use egui::{Align2, CornerRadius, Rect, Role, Sense, StrokeKind, WidgetInfo};
use platter_engine::{Command, CrossfaderCurve, MixerMode, Output};

use crate::command::{Action, AppCommand, RouteSlot, SettingsPage};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::i18n::{Language, Texts};
use crate::keymap::Chord;
use crate::model::{AudioState, View};
use crate::paint;
use crate::panels::header::problem_keys;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::key::{self, KeySpec, Press};

pub const DIALOG_SIZE: egui::Vec2 = egui::vec2(880.0, 620.0);
const NAV_WIDTH: f32 = 200.0;
const FOOTER_HEIGHT: f32 = 53.0;
/// Left edge of the names of the settings and of their controls, from the left of the page.
const LABEL_X: f32 = 24.0;
const CONTROL_X: f32 = 208.0;
const ROW: f32 = 27.0;

/// A chord as the interface writes it: each part in the language of the user.
pub fn chord_text(texts: &Texts, chord: &Chord) -> String {
    chord
        .parts()
        .iter()
        .map(|part| {
            let key = format!("key.{}", part.to_ascii_lowercase());
            let text = texts.get(&key);
            // Letters, digits and function keys have no translation and show as they are.
            if text == key {
                (*part).to_owned()
            } else {
                text.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(texts.get("key.joiner"))
}

/// The label of a command in the shortcut list: its text with the deck it acts on.
fn binding_label(texts: &Texts, command: &str, args: &platter_engine::command::Args) -> String {
    let text = |name: &str| match args.get(name) {
        Some(platter_engine::command::Value::Text(text)) => text.to_uppercase(),
        _ => String::new(),
    };
    let band = match args.get("band") {
        Some(platter_engine::command::Value::Text(band)) => {
            texts.get(&format!("mixer.{band}")).to_lowercase()
        }
        _ => String::new(),
    };
    // The number of a hotcue, without the decimals a number of the file may carry.
    let slot = match args.get("slot") {
        Some(platter_engine::command::Value::Number(slot)) => format!("{slot:.0}"),
        _ => String::new(),
    };
    // The two bend keys share a command; the sign of its amount tells them apart.
    let key = match (command, args.get("amount")) {
        ("deck.nudge", Some(platter_engine::command::Value::Number(amount))) if *amount < 0.0 => {
            "command.deck.nudge.slower".to_owned()
        }
        ("deck.nudge", Some(_)) => "command.deck.nudge.faster".to_owned(),
        _ => format!("command.{command}"),
    };
    texts.format(
        &key,
        &[("deck", &text("deck")), ("band", &band), ("slot", &slot)],
    )
}

/// What the dialog keeps between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SettingsMemory {
    pub shortcuts_scroll: f32,
}

pub fn settings(
    cx: &mut Cx<'_>,
    rect: Rect,
    page: SettingsPage,
    view: &View<'_>,
    memory: &mut SettingsMemory,
) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect(
        rect.expand(1.0),
        CornerRadius::ZERO,
        color.bg_app,
        paint::hairline(theme, color.line_key),
        StrokeKind::Outside,
    );
    let nav = Rect::from_min_size(rect.min, egui::vec2(NAV_WIDTH, rect.height()));
    let content = Rect::from_min_max(
        egui::pos2(nav.right() + tokens.space.s1, rect.top()),
        rect.max,
    );
    painter.rect_filled(nav, CornerRadius::ZERO, color.surface_panel);
    painter.rect_filled(content, CornerRadius::ZERO, color.surface_panel);

    let heading = Rect::from_min_size(
        nav.min + egui::vec2(18.0, 12.0),
        egui::vec2(NAV_WIDTH - 26.0, 26.0),
    );
    paint::text(
        painter,
        theme,
        heading,
        Align2::LEFT_CENTER,
        texts.get("settings.title"),
        tokens.text.title,
        color.text_primary,
    );
    for (index, entry) in SettingsPage::ALL.into_iter().enumerate() {
        let item = Rect::from_min_size(
            nav.min + egui::vec2(8.0, 47.0 + 29.0 * index as f32),
            egui::vec2(NAV_WIDTH - 16.0, 28.0),
        );
        let selected = entry == page;
        let action = AppCommand::SettingsOpen { page: entry };
        cx.record(action.id(), item, ControlKind::Button, true);
        let response = cx.ui.interact(
            item,
            id(("settings-nav", index)),
            cx.sense(true, Sense::click()),
        );
        let name = texts.get(match entry {
            SettingsPage::Audio => "settings.audio",
            SettingsPage::General => "settings.general",
            SettingsPage::Mixer => "settings.mixer",
        });
        response.widget_info(|| WidgetInfo::selected(Role::Tab, true, selected, name));
        if response.clicked() {
            cx.out.emit(action);
        }
        if selected {
            painter.rect_filled(
                item,
                paint::radius(tokens.radius.md),
                color.surface_selected,
            );
        } else if response.hovered() {
            painter.rect_filled(item, paint::radius(tokens.radius.md), color.surface_hover);
        }
        let ink = if selected {
            color.text_primary
        } else {
            color.text_secondary
        };
        let label = item.shrink2(egui::vec2(10.0, 0.0));
        paint::text(
            painter,
            theme,
            label,
            Align2::LEFT_CENTER,
            name,
            tokens.text.body,
            ink,
        );
        if response.has_focus() {
            cx.out.claim(&[egui::Key::Space, egui::Key::Enter]);
            paint::focus_ring(painter, theme, item, tokens.radius.md);
        }
    }

    let page_rect = Rect::from_min_max(
        content.min,
        egui::pos2(content.right(), content.bottom() - FOOTER_HEIGHT),
    );
    match page {
        SettingsPage::Audio => audio_page(cx, page_rect, view),
        SettingsPage::General => general_page(cx, page_rect, view, memory),
        SettingsPage::Mixer => mixer_page(cx, page_rect, view),
    }
    footer(
        cx,
        Rect::from_min_max(page_rect.left_bottom(), content.max),
        view,
    );
}

/// The heading of a group of settings, with its rule. Returns the top of its first row.
fn section(cx: &Cx<'_>, page: Rect, top: f32, title: &str) -> f32 {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let heading = Rect::from_min_max(
        egui::pos2(page.left() + LABEL_X, page.top() + top),
        egui::pos2(page.right() - LABEL_X, page.top() + top + ROW),
    );
    paint::text(
        painter,
        theme,
        heading,
        Align2::LEFT_TOP,
        cx.texts.get(title),
        tokens.text.title,
        color.text_primary,
    );
    painter.hline(
        heading.x_range(),
        heading.bottom() - 0.5,
        paint::hairline(theme, color.line_divider),
    );
    top + ROW + 10.0
}

/// The name of a setting at the left of its row. Returns the area for its control.
fn setting(cx: &Cx<'_>, page: Rect, top: f32, name: &str) -> Rect {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let label = Rect::from_min_size(
        egui::pos2(page.left() + LABEL_X, page.top() + top),
        egui::vec2(CONTROL_X - LABEL_X - tokens.space.s5, tokens.size.button_h),
    );
    paint::text_elided(
        cx.painter(),
        theme,
        label,
        cx.texts.get(name),
        tokens.text.body,
        tokens.color.text_secondary,
    );
    Rect::from_min_max(
        egui::pos2(page.left() + CONTROL_X, page.top() + top),
        egui::pos2(
            page.right() - LABEL_X,
            page.top() + top + tokens.size.button_h,
        ),
    )
}

/// A line of help under a control.
fn help(cx: &Cx<'_>, control: Rect, text: &str) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let line = Rect::from_min_size(
        egui::pos2(control.left(), control.bottom() + tokens.space.s2),
        egui::vec2(control.width(), 14.0),
    );
    paint::text_elided(
        cx.painter(),
        theme,
        line,
        text,
        tokens.text.readout,
        tokens.color.text_secondary,
    );
}

fn audio_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let audio = view.audio;
    let settings = view.settings;
    let stream = audio.stream();

    let mut top = section(cx, page, 20.0, "settings.device");
    // The device: the default of the system first, then the devices by name.
    let control = setting(cx, page, top, "settings.audio_device");
    let default_label = texts.get("settings.default_device");
    let mut labels = vec![default_label];
    labels.extend(audio.devices.iter().map(|device| device.name.as_str()));
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let current = match &settings.device {
        None => Some(0),
        Some(name) => audio
            .devices
            .iter()
            .position(|device| &device.name == name)
            .map(|index| index + 1),
    };
    let chosen = current
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| audio.devices.get(index));
    let devices = &audio.devices;
    let select_rect = Rect::from_min_size(control.min, egui::vec2(280.0, tokens.size.button_h));
    let detail = match (chosen, stream) {
        (Some(device), _) if device.inputs > 0 => texts.format(
            "settings.device_channels",
            &[
                ("outputs", &device.outputs.to_string()),
                ("inputs", &device.inputs.to_string()),
            ],
        ),
        (Some(device), _) => texts.format(
            "settings.device_outputs",
            &[("outputs", &device.outputs.to_string())],
        ),
        (None, Some(stream)) => texts.format(
            "settings.device_outputs",
            &[("outputs", &stream.channels.to_string())],
        ),
        (None, None) => String::new(),
    };
    help(cx, select_rect, &detail);

    top += 56.0;
    let control = setting(cx, page, top, "settings.sample_rate");
    let supported = |rate: u32| chosen.is_none_or(|device| device.sample_rates.contains(&rate));
    let rate_labels: Vec<String> = audio
        .sample_rates
        .iter()
        .map(|rate| {
            texts.format(
                "settings.rate_value",
                &[("value", &format::kilohertz(*rate))],
            )
        })
        .collect();
    let rate_choices: Vec<Choice<'_>> = rate_labels
        .iter()
        .zip(&audio.sample_rates)
        .map(|(label, rate)| Choice {
            label,
            enabled: supported(*rate),
        })
        .collect();
    let rates = &audio.sample_rates;
    choice::segmented(
        cx,
        control,
        id("settings-rate"),
        &rate_choices,
        rates.iter().position(|rate| *rate == settings.sample_rate),
        |index| AppCommand::AudioSampleRateSet { hz: rates[index] }.into(),
    );

    top += 38.0;
    let control = setting(cx, page, top, "settings.buffer_size");
    let buffer_labels: Vec<String> = audio.buffer_sizes.iter().map(u32::to_string).collect();
    let buffer_choices: Vec<Choice<'_>> = buffer_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let buffers = &audio.buffer_sizes;
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-buffer"),
        &buffer_choices,
        buffers
            .iter()
            .position(|frames| *frames == settings.buffer_frames),
        |index| {
            AppCommand::AudioBufferSet {
                frames: buffers[index],
            }
            .into()
        },
    );
    let buffer_help = match stream {
        Some(stream) if stream.buffer_frames != settings.buffer_frames => texts.format(
            "settings.buffer_actual",
            &[("frames", &stream.buffer_frames.to_string())],
        ),
        _ => texts.get("settings.buffer_help").to_owned(),
    };
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        &buffer_help,
    );

    top += 56.0;
    let control = setting(cx, page, top, "settings.status");
    let painter = cx.painter();
    let (dot, status) = match &audio.state {
        AudioState::Running(stream) => (
            color.status_ok,
            texts.format(
                "settings.status_running",
                &[
                    ("latency", &format!("{:.1}", stream.latency_ms)),
                    ("dropouts", &stream.dropouts.to_string()),
                ],
            ),
        ),
        AudioState::Stopped(problem) => {
            let (title, text) = problem_keys(*problem);
            (
                color.status_error,
                texts.format(
                    "settings.status_stopped",
                    &[("title", texts.get(title)), ("text", texts.get(text))],
                ),
            )
        }
    };
    painter.circle_filled(
        egui::pos2(control.left() + 4.0, control.center().y),
        4.0,
        dot,
    );
    let status_rect = Rect::from_min_max(
        egui::pos2(control.left() + 16.0, control.top()),
        control.max,
    );
    paint::text_elided(
        painter,
        theme,
        status_rect,
        &status,
        tokens.text.readout,
        color.text_primary,
    );
    let response = cx
        .ui
        .interact(status_rect, id("settings-status"), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Status, true, &status));

    top = section(cx, page, top + 47.0, "settings.routing");
    // Outputs of the device, by number. The master may use any; the headphones need a
    // device with a second pair.
    let outputs = stream.map_or(2, |stream| stream.channels).max(2);
    let output_labels: Vec<String> = (1..=outputs)
        .map(|number| texts.format("settings.output", &[("number", &number.to_string())]))
        .collect();
    let output_choices: Vec<Choice<'_>> = output_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let pairs = [
        (
            "settings.master",
            [RouteSlot::MasterLeft, RouteSlot::MasterRight],
            true,
        ),
        (
            "settings.headphones",
            [RouteSlot::HeadphonesLeft, RouteSlot::HeadphonesRight],
            outputs >= 4,
        ),
    ];
    let mut routing_selects = Vec::new();
    for (name, slots, enabled) in pairs {
        let control = setting(cx, page, top, name);
        for (index, slot) in slots.into_iter().enumerate() {
            let rect = Rect::from_min_size(
                control.min + egui::vec2(144.0 * index as f32, 0.0),
                egui::vec2(136.0, tokens.size.button_h),
            );
            routing_selects.push((rect, slot, enabled));
        }
        if !enabled {
            help(cx, control, texts.get("settings.routing_help"));
        }
        top += if enabled { 38.0 } else { 56.0 };
    }

    let control = setting(cx, page, top, "settings.test_tone");
    let mut left = control.left();
    for (label, output, enabled) in [
        ("settings.tone_master", Output::Master, stream.is_some()),
        (
            "settings.tone_headphones",
            Output::Headphones,
            audio.has_headphones(),
        ),
    ] {
        let label = texts.get(label);
        let width = key::width_for(cx, label, 0.0);
        let rect = Rect::from_min_size(
            egui::pos2(left, control.top()),
            egui::vec2(width, tokens.size.button_h),
        );
        left += width + tokens.space.s3;
        key::key(
            cx,
            rect,
            id(("settings-tone", output.name())),
            KeySpec::text(label).enabled(enabled),
            Press::Click(Command::MasterTestTone { output }.into()),
        );
    }

    // The drop-downs last, so an open list lies over the controls below it.
    for (rect, slot, enabled) in routing_selects.into_iter().rev() {
        let name = texts.get(match slot {
            RouteSlot::MasterLeft => "settings.route_master_left",
            RouteSlot::MasterRight => "settings.route_master_right",
            RouteSlot::HeadphonesLeft => "settings.route_headphones_left",
            RouteSlot::HeadphonesRight => "settings.route_headphones_right",
        });
        let current = usize::from(settings.routing[slot.index()]);
        choice::select(
            cx,
            rect,
            id(("settings-route", slot.index())),
            SelectSpec {
                name,
                choices: &output_choices,
                current: (current < output_choices.len()).then_some(current),
                enabled,
            },
            |index| {
                AppCommand::AudioRoutingSet {
                    slot,
                    output: index as u16,
                }
                .into()
            },
        );
    }
    choice::select(
        cx,
        select_rect,
        id("settings-device"),
        SelectSpec {
            name: texts.get("settings.audio_device"),
            choices: &choices,
            current,
            enabled: true,
        },
        |index| {
            let device = index.checked_sub(1).and_then(|index| devices.get(index));
            AppCommand::AudioDeviceSet {
                device: device.map(|device| device.name.clone()),
            }
            .into()
        },
    );
}

fn general_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>, memory: &mut SettingsMemory) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;

    let top = section(cx, page, 20.0, "settings.language_section");
    let language_control = setting(cx, page, top, "settings.language");
    let language_rect = Rect::from_min_size(
        language_control.min,
        egui::vec2(280.0, tokens.size.button_h),
    );
    help(cx, language_control, texts.get("settings.language_help"));

    let top = section(cx, page, 127.0, "settings.mixer_section");
    let control = setting(cx, page, top, "settings.crossfader_curve");
    let curve_labels = CrossfaderCurve::ALL.map(|curve| {
        texts.get(match curve {
            CrossfaderCurve::Smooth => "settings.curve_smooth",
            CrossfaderCurve::Linear => "settings.curve_linear",
            CrossfaderCurve::Sharp => "settings.curve_sharp",
        })
    });
    let curve_choices: Vec<Choice<'_>> = curve_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let current = view.engine.mixer.crossfader_curve;
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-curve"),
        &curve_choices,
        CrossfaderCurve::ALL
            .iter()
            .position(|curve| *curve == current),
        |index| {
            Action::Engine(Command::MixerCrossfaderCurveSet {
                curve: CrossfaderCurve::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.curve_help"),
    );

    let top = section(cx, page, 234.0, "settings.keyboard_section");
    let painter = cx.painter();
    let table = Rect::from_min_max(
        egui::pos2(page.left() + LABEL_X, page.top() + top),
        egui::pos2(page.right() - LABEL_X, page.bottom() - 60.0),
    );
    let command_x = table.left() + tokens.space.s3;
    let shortcut_x = table.right() - 168.0;
    let header = Rect::from_min_size(table.min, egui::vec2(table.width(), ROW));
    for (x, name) in [
        (command_x, "settings.command"),
        (shortcut_x, "settings.shortcut"),
    ] {
        let cell = Rect::from_min_max(egui::pos2(x, header.top()), header.max);
        paint::text(
            painter,
            theme,
            cell,
            Align2::LEFT_CENTER,
            texts.get(name),
            tokens.text.label,
            color.text_secondary,
        );
    }
    painter.hline(
        table.x_range(),
        header.bottom() - 0.5,
        paint::hairline(theme, color.line_divider),
    );

    // The list is read-only: it scrolls and nothing in it can be changed here.
    let rows = Rect::from_min_max(header.left_bottom(), table.max);
    let bindings = view.keymap.bindings();
    let content = bindings.len() as f32 * ROW;
    let response = cx.ui.interact(
        rows,
        id("settings-shortcuts"),
        cx.sense(true, Sense::click()),
    );
    let name = texts.get("settings.keyboard_section");
    response.widget_info(|| WidgetInfo::labeled(Role::List, true, name));
    let mut scroll = memory.shortcuts_scroll;
    if response.hovered() {
        scroll -= cx.ui.input(|input| input.smooth_scroll_delta.y);
    }
    if response.has_focus() {
        let filter = egui::EventFilter {
            vertical_arrows: true,
            ..egui::EventFilter::default()
        };
        cx.ui
            .memory_mut(|memory| memory.set_focus_lock_filter(response.id, filter));
        cx.out.claim(&[egui::Key::ArrowUp, egui::Key::ArrowDown]);
        let (up, down) = cx.ui.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::ArrowDown),
            )
        });
        scroll += ROW * (f32::from(u8::from(down)) - f32::from(u8::from(up)));
        paint::focus_ring(
            painter,
            theme,
            rows.shrink(tokens.space.s1 + tokens.stroke.focus),
            tokens.radius.md,
        );
    }
    memory.shortcuts_scroll = scroll.clamp(0.0, (content - rows.height()).max(0.0));
    let clipped = painter.with_clip_rect(rows);
    for (index, binding) in bindings.iter().enumerate() {
        let row = Rect::from_min_size(
            egui::pos2(
                rows.left(),
                rows.top() + index as f32 * ROW - memory.shortcuts_scroll,
            ),
            egui::vec2(rows.width(), ROW),
        );
        if !rows.intersects(row) {
            continue;
        }
        let label = binding_label(texts, binding.command, &binding.args);
        let command = Rect::from_min_max(
            egui::pos2(command_x, row.top()),
            egui::pos2(shortcut_x - tokens.space.s4, row.bottom()),
        );
        paint::text_elided(
            &clipped,
            theme,
            command,
            &label,
            tokens.text.body,
            color.text_primary,
        );
        let shortcut = Rect::from_min_max(egui::pos2(shortcut_x, row.top()), row.max);
        paint::text_elided(
            &clipped,
            theme,
            shortcut,
            &chord_text(texts, &binding.chord),
            tokens.text.body,
            color.text_primary,
        );
        clipped.hline(
            row.x_range(),
            row.bottom() - 0.5,
            paint::hairline(theme, color.line_divider),
        );
    }

    let line = Rect::from_min_max(
        egui::pos2(table.left(), table.bottom() + 16.0),
        egui::pos2(table.right(), table.bottom() + 16.0 + tokens.size.button_h),
    );
    let open = texts.format(
        "settings.open_file",
        &[("file", &view.settings.keymap_file)],
    );
    let width = key::width_for(cx, &open, 0.0);
    let button = Rect::from_min_size(
        egui::pos2(line.right() - width, line.top()),
        egui::vec2(width, line.height()),
    );
    let count = texts.format(
        "settings.shortcut_count",
        &[
            ("count", &bindings.len().to_string()),
            ("file", &view.settings.keymap_file),
        ],
    );
    let text = Rect::from_min_max(
        line.min,
        egui::pos2(button.left() - tokens.space.s3, line.bottom()),
    );
    paint::text_elided(
        cx.painter(),
        theme,
        text,
        &count,
        tokens.text.readout,
        color.text_secondary,
    );
    key::key(
        cx,
        button,
        id("settings-open-keymap"),
        KeySpec::text(&open),
        Press::Click(AppCommand::KeymapOpen.into()),
    );

    // The language drop-down last, so its open list lies over what is below it.
    let names = Language::ALL.map(|language| {
        texts.get(match language {
            Language::English => "language.en",
            Language::Spanish => "language.es",
        })
    });
    let choices: Vec<Choice<'_>> = names
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    choice::select(
        cx,
        language_rect,
        id("settings-language"),
        SelectSpec {
            name: texts.get("settings.language"),
            choices: &choices,
            current: Language::ALL
                .iter()
                .position(|language| *language == view.settings.language),
            enabled: true,
        },
        |index| {
            AppCommand::LanguageSet {
                language: Language::ALL[index],
            }
            .into()
        },
    );
}

/// Lengths of an automatic fade the Mixer page offers, in beats.
const FADE_BEATS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];

fn mixer_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let texts = cx.texts;
    let mixer = &view.engine.mixer;

    let top = section(cx, page, 20.0, "settings.mixing_section");
    let control = setting(cx, page, top, "settings.mixing_mode");
    let mode_labels = MixerMode::ALL.map(|mode| {
        texts.get(match mode {
            MixerMode::Internal => "settings.mode_internal",
            MixerMode::External => "settings.mode_external",
        })
    });
    let mode_choices: Vec<Choice<'_>> = mode_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-mixer-mode"),
        &mode_choices,
        MixerMode::ALL.iter().position(|mode| *mode == mixer.mode),
        |index| {
            Action::Engine(Command::MixerModeSet {
                mode: MixerMode::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.mode_help"),
    );
    switch(
        cx,
        page,
        top + 50.0,
        Switch {
            name: "settings.autogain",
            help: "settings.autogain_help",
            on: mixer.autogain,
            command: Command::MixerAutogainToggle,
        },
    );

    let top = section(cx, page, 175.0, "settings.external_section");
    switch(
        cx,
        page,
        top,
        Switch {
            name: "settings.external_eq_gain",
            help: "settings.external_help",
            on: mixer.external_eq_gain,
            command: Command::MixerExternalEqGainToggle,
        },
    );
    switch(
        cx,
        page,
        top + 50.0,
        Switch {
            name: "settings.external_master_volume",
            help: "settings.external_help",
            on: mixer.external_master_volume,
            command: Command::MixerExternalMasterVolumeToggle,
        },
    );

    let top = section(cx, page, 330.0, "settings.crossfader_section");
    let control = setting(cx, page, top, "settings.fade_length");
    let fade_labels = FADE_BEATS.map(|beats| beats.to_string());
    let fade_choices: Vec<Choice<'_>> = fade_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-fade-length"),
        &fade_choices,
        FADE_BEATS
            .iter()
            .position(|beats| f64::from(*beats) == mixer.fade_beats),
        |index| {
            Action::Engine(Command::MixerCrossfaderFadeBeatsSet {
                beats: f64::from(FADE_BEATS[index]),
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.fade_help"),
    );
}

/// A setting that is on or off: a key that toggles it, lit while it is on.
struct Switch {
    name: &'static str,
    help: &'static str,
    on: bool,
    command: Command,
}

fn switch(cx: &mut Cx<'_>, page: Rect, top: f32, switch: Switch) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let control = setting(cx, page, top, switch.name);
    let state = texts.get(if switch.on {
        "settings.on"
    } else {
        "settings.off"
    });
    let width = key::width_for(cx, state, 72.0);
    let rect = Rect::from_min_size(control.min, egui::vec2(width, tokens.size.button_h));
    let mut spec = KeySpec::text(state);
    spec.name = texts.get(switch.name);
    spec.lit = switch.on.then_some(tokens.color.text_primary);
    key::key(
        cx,
        rect,
        id(("settings-switch", switch.name)),
        spec,
        Press::Click(switch.command.into()),
    );
    help(
        cx,
        Rect::from_min_max(rect.min, egui::pos2(control.right(), rect.bottom())),
        texts.get(switch.help),
    );
}

fn footer(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    cx.painter().hline(
        rect.x_range(),
        rect.top() + 0.5,
        paint::hairline(theme, color.line_divider),
    );
    let top = rect.center().y - tokens.size.button_h / 2.0;
    let mut right = rect.right() - tokens.space.s5;
    for (label, action, key_id) in [
        (
            "settings.close",
            AppCommand::SettingsClose,
            "settings-close",
        ),
        (
            "settings.restore",
            AppCommand::SettingsRestoreDefaults,
            "settings-restore",
        ),
    ] {
        let label = texts.get(label);
        let width = key::width_for(cx, label, 80.0);
        let key_rect = Rect::from_min_size(
            egui::pos2(right - width, top),
            egui::vec2(width, tokens.size.button_h),
        );
        right = key_rect.left() - tokens.space.s3;
        key::key(
            cx,
            key_rect,
            id(key_id),
            KeySpec::text(label),
            Press::Click(action.into()),
        );
    }
    let note = texts.format(
        "settings.saved_note",
        &[("file", &view.settings.settings_file)],
    );
    let area = Rect::from_min_max(
        egui::pos2(rect.left() + tokens.space.s5, rect.top()),
        egui::pos2(right, rect.bottom()),
    );
    paint::text_elided(
        cx.painter(),
        theme,
        area,
        &note,
        tokens.text.readout,
        color.text_secondary,
    );
}
