//! The preferences dialog: the Audio, General and Mixer pages.
//!
//! Component: `Select` in `docs/design/DESIGN.md`; placement from
//! `docs/design/screens/Preferences-audio.html` and `Preferences-general.html`; the Mixer
//! page follows the form pattern of the Audio page, as the design system asks. Behaviour
//! note: `docs/behavior/settings.md`.

use egui::{Align2, CornerRadius, Rect, Role, Sense, StrokeKind, WidgetInfo};
use platter_core::DeckId;
use platter_engine::command::{CommandSpec, ParamKind, SPECS};
use platter_engine::{
    Command, CrossfaderCurve, EqModel, FilterModel, FxRouting, FxUnit, MixerMode, Output, SyncMode,
};

use crate::collection::TagWriting;
use crate::command::{
    Action, AppCommand, DeckLayout, DeckSize, EffectUnits, RouteSlot, SettingsPage, WaveColour,
};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::i18n::{Language, Texts};
use crate::keymap::Chord;
use crate::keys::KeyNotation;
use crate::model::{AudioState, ControlType, View};
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
    /// How far the Mixer page is scrolled.
    pub page_scroll: f32,
    /// How far the Decks page is scrolled.
    pub decks_scroll: f32,
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
            SettingsPage::Decks => "settings.decks",
            SettingsPage::Mixer => "settings.mixer",
            SettingsPage::Library => "settings.library",
            SettingsPage::Controllers => "settings.controllers",
            SettingsPage::Keyboard => "settings.keyboard",
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
        SettingsPage::General => general_page(cx, page_rect, view),
        SettingsPage::Keyboard => keyboard_page(cx, page_rect, view, memory),
        SettingsPage::Decks => scrolled(
            cx,
            page_rect,
            (DECKS_PAGE_HEIGHT, &mut memory.decks_scroll),
            |cx, page| decks_page(cx, page, view),
        ),
        SettingsPage::Mixer => scrolled(
            cx,
            page_rect,
            (MIXER_PAGE_HEIGHT, &mut memory.page_scroll),
            |cx, page| mixer_page(cx, page, view),
        ),
        SettingsPage::Library => library_page(cx, page_rect, view),
        SettingsPage::Controllers => controllers_page(cx, page_rect, view),
    }
    footer(
        cx,
        Rect::from_min_max(page_rect.left_bottom(), content.max),
        view,
    );
}

/// The height of the content of the Mixer page, which scrolls.
const MIXER_PAGE_HEIGHT: f32 = 960.0;
/// The height of the content of the Decks page, which scrolls.
const DECKS_PAGE_HEIGHT: f32 = 600.0;

/// Draws a page taller than its room, scrolled by `scroll`: the wheel over it scrolls it, and
/// a control of the page the keyboard focus reaches out of view is brought into view.
fn scrolled(
    cx: &mut Cx<'_>,
    rect: Rect,
    (content, scroll): (f32, &mut f32),
    draw: impl FnOnce(&mut Cx<'_>, Rect),
) {
    let ctx = cx.ui.ctx().clone();
    let clip = rect.intersect(cx.ui.clip_rect());
    if cx.interactive && cx.ui.rect_contains_pointer(clip) {
        *scroll -= cx.ui.input(|input| input.smooth_scroll_delta.y);
    }
    // A control of the page is clipped by it; the keys of the footer and the list of pages
    // are not, so only the controls of the page move it.
    if let Some(focused) = ctx
        .memory(|memory| memory.focused())
        .and_then(|id| ctx.read_response(id))
        .filter(|response| response.interact_rect != response.rect)
        .filter(|response| {
            response.rect.left() >= rect.left() && response.rect.right() <= rect.right()
        })
    {
        let margin = cx.theme.tokens.space.s4;
        if focused.rect.bottom() > clip.bottom() {
            *scroll += focused.rect.bottom() - clip.bottom() + margin;
        } else if focused.rect.top() < clip.top() {
            *scroll -= clip.top() - focused.rect.top() + margin;
        }
    }
    *scroll = scroll.clamp(0.0, (content - rect.height()).max(0.0));
    let mut ui = egui::Ui::new(
        ctx,
        cx.ui.scope_id().with("scrolled-page"),
        egui::UiBuilder::new()
            .layer_id(cx.ui.layer_id())
            .max_rect(rect),
    );
    ui.set_clip_rect(clip);
    let page = rect.translate(egui::vec2(0.0, -*scroll));
    draw(&mut cx.with_ui(&ui), page);
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
    setting_named(cx, page, top, cx.texts.get(name))
}

/// [`setting`] with a name already translated.
fn setting_named(cx: &Cx<'_>, page: Rect, top: f32, name: &str) -> Rect {
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
        name,
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

fn general_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;

    let top = section(cx, page, 20.0, "settings.language_section");
    let language_control = setting(cx, page, top, "settings.language");
    let language_rect = Rect::from_min_size(
        language_control.min,
        egui::vec2(280.0, tokens.size.button_h),
    );
    help(cx, language_control, texts.get("settings.language_help"));
    switch(
        cx,
        page,
        top + 50.0,
        Switch {
            name: "settings.tooltips",
            help: "settings.tooltips_help",
            on: view.settings.tooltips,
            action: AppCommand::TooltipsToggle.into(),
        },
    );
    switch(
        cx,
        page,
        top + 100.0,
        Switch {
            name: "settings.fullscreen_at_start",
            help: "settings.fullscreen_at_start_help",
            on: view.settings.fullscreen_at_start,
            action: AppCommand::FullscreenAtStartToggle.into(),
        },
    );

    let top = section(cx, page, 210.0, "settings.layout_section");
    let control = setting(cx, page, top, "settings.deck_layout");
    let labels = DeckLayout::ALL.map(|layout| {
        texts.get(match layout {
            DeckLayout::Two => "settings.decks_two",
            DeckLayout::Four => "settings.decks_four",
        })
    });
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-deck-layout"),
        &choices,
        DeckLayout::ALL
            .iter()
            .position(|layout| *layout == view.settings.deck_layout),
        |index| {
            Action::App(AppCommand::DeckLayoutSet {
                layout: DeckLayout::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.deck_layout_help"),
    );

    let top = top + 50.0;
    let control = setting(cx, page, top, "settings.deck_size");
    let labels = DeckSize::ALL.map(|size| {
        texts.get(match size {
            DeckSize::Full => "settings.deck_full",
            DeckSize::Compact => "settings.deck_compact",
        })
    });
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-deck-size"),
        &choices,
        DeckSize::ALL
            .iter()
            .position(|size| *size == view.settings.deck_size),
        |index| {
            Action::App(AppCommand::DeckSizeSet {
                size: DeckSize::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.deck_size_help"),
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

/// The shortcuts of the keyboard file, read-only: they are changed in the file.
fn keyboard_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>, memory: &mut SettingsMemory) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;

    let top = section(cx, page, 20.0, "settings.keyboard_section");
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
}

/// Lengths of an automatic fade the Mixer page offers, in beats.
const FADE_BEATS: [u32; 7] = [1, 2, 4, 8, 16, 32, 64];

fn decks_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let texts = cx.texts;
    let clock = &view.engine.clock;
    let top = section(cx, page, 20.0, "settings.sync_section");
    let control = setting(cx, page, top, "settings.sync_mode");
    let labels = SyncMode::ALL.map(|mode| {
        texts.get(match mode {
            SyncMode::Tempo => "settings.sync_tempo",
            SyncMode::Beat => "settings.sync_beat",
        })
    });
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-sync-mode"),
        &choices,
        SyncMode::ALL
            .iter()
            .position(|mode| *mode == clock.sync_mode),
        |index| {
            Action::Engine(Command::GlobalSyncModeSet {
                mode: SyncMode::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.sync_help"),
    );

    let control = setting(cx, page, top + 50.0, "settings.tempo_range");
    let labels = TEMPO_RANGE_PERCENTS.map(|percent| format!("{percent} %"));
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-tempo-range"),
        &choices,
        TEMPO_RANGE_PERCENTS
            .iter()
            .position(|percent| *percent == view.settings.tempo_range),
        |index| {
            Action::App(AppCommand::TempoRangeSet {
                percent: TEMPO_RANGE_PERCENTS[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.tempo_range_help"),
    );

    let top = section(cx, page, 175.0, "settings.keys_section");
    let control = setting(cx, page, top, "settings.key_notation");
    let notation = view.settings.key_notation;
    let labels = KeyNotation::ALL.map(|choice| {
        texts.get(match choice {
            KeyNotation::Flats => "settings.notation_flats",
            KeyNotation::Sharps => "settings.notation_sharps",
            KeyNotation::OpenKey => "settings.notation_open_key",
        })
    });
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-key-notation"),
        &choices,
        KeyNotation::ALL
            .iter()
            .position(|choice| *choice == notation),
        |index| {
            Action::App(AppCommand::KeyNotationSet {
                notation: KeyNotation::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.key_notation_help"),
    );

    let top = section(cx, page, 280.0, "settings.loading_section");
    switch(
        cx,
        page,
        top,
        Switch {
            name: "settings.load_marker",
            help: "settings.load_marker_help",
            on: view.engine.load_marker,
            action: Command::GlobalLoadMarkerToggle.into(),
        },
    );
    switch(
        cx,
        page,
        top + 50.0,
        Switch {
            name: "settings.stopped_only",
            help: "settings.stopped_only_help",
            on: view.settings.load_stopped_only,
            action: AppCommand::LoadStoppedOnlyToggle.into(),
        },
    );

    let top = section(cx, page, 395.0, "settings.analysis_section");
    let control = setting(cx, page, top, "settings.bpm_range");
    let range = view.settings.bpm_range;
    let labels = TEMPO_RANGES.map(|(min, max)| format!("{min}-{max}"));
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let select = Rect::from_min_size(control.min, egui::vec2(120.0, control.height()));
    choice::select(
        cx,
        select,
        id("settings-bpm-range"),
        SelectSpec {
            name: texts.get("settings.bpm_range"),
            choices: &choices,
            current: TEMPO_RANGES.iter().position(|ends| *ends == range),
            enabled: true,
        },
        |index| {
            let (min, max) = TEMPO_RANGES[index];
            AppCommand::AnalysisRangeSet { min, max }.into()
        },
    );
    help(
        cx,
        Rect::from_min_max(select.min, egui::pos2(control.right(), select.bottom())),
        texts.get("settings.bpm_range_help"),
    );

    // `docs/behavior/waveform.md`, *Colour modes*.
    let top = section(cx, page, 500.0, "settings.waveform_section");
    let control = setting(cx, page, top, "settings.wave_colour");
    let labels = WaveColour::ALL.map(|mode| {
        texts.get(match mode {
            WaveColour::Deck => "settings.wave_deck",
            WaveColour::Bands => "settings.wave_bands",
            WaveColour::Mono => "settings.wave_mono",
            WaveColour::Bass => "settings.wave_bass",
        })
    });
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let select = Rect::from_min_size(control.min, egui::vec2(120.0, control.height()));
    let current = view.settings.wave_colour;
    choice::select(
        cx,
        select,
        id("settings-wave-colour"),
        SelectSpec {
            name: texts.get("settings.wave_colour"),
            choices: &choices,
            current: WaveColour::ALL.iter().position(|mode| *mode == current),
            enabled: true,
        },
        |index| {
            AppCommand::WaveColourSet {
                mode: WaveColour::ALL[index],
            }
            .into()
        },
    );
    help(
        cx,
        Rect::from_min_max(select.min, egui::pos2(control.right(), select.bottom())),
        texts.get("settings.wave_colour_help"),
    );
}

/// The ranges of the tempo faders the Decks page offers, in percent: those of the deck.
const TEMPO_RANGE_PERCENTS: [u32; 3] = [8, 16, 50];

/// The ranges of tempos the Decks page offers, in BPM: the wide default, then one octave each.
const TEMPO_RANGES: [(u32, u32); 7] = [
    (70, 180),
    (60, 120),
    (70, 140),
    (80, 160),
    (90, 180),
    (100, 200),
    (120, 240),
];

/// Height of a row of the table of assignments.
const BINDING_ROW: f32 = 28.0;
/// Most assignments the table shows; the rest are reached by selecting from the keyboard
/// file or by deleting.
const BINDING_ROWS: usize = 9;

/// The controller manager: the device, its input and its mapping, then the table of the
/// assignments of the mapping with the keys that edit it. `docs/behavior/controllers.md`.
fn controllers_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let manager = &view.settings.controllers;
    let index = manager
        .selected
        .filter(|index| *index < manager.controllers.len());
    let controller = index.and_then(|index| manager.controllers.get(index));
    let gap = tokens.space.s3;

    let mut top = section(cx, page, 20.0, "settings.midi_device");
    // The controller: a list of the controllers of the settings, Add and Remove.
    let control = setting(cx, page, top, "settings.midi_controller");
    let labels: Vec<&str> = manager
        .controllers
        .iter()
        .map(|controller| controller.input.as_str())
        .collect();
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let select = Rect::from_min_size(control.min, egui::vec2(240.0, tokens.size.button_h));
    choice::select(
        cx,
        select,
        id("settings-midi-controller"),
        SelectSpec {
            name: texts.get("settings.midi_controller"),
            choices: &choices,
            current: index,
            enabled: !choices.is_empty(),
        },
        |index| AppCommand::MidiControllerSelect { index }.into(),
    );
    let mut x = select.right() + gap;
    for (key_id, label, action, enabled) in [
        (
            "settings-midi-add",
            "settings.midi_add",
            Action::App(AppCommand::MidiControllerAdd),
            !manager.inputs.is_empty(),
        ),
        (
            "settings-midi-remove",
            "settings.midi_remove",
            Action::App(AppCommand::MidiControllerRemove {
                index: index.unwrap_or(0),
            }),
            controller.is_some(),
        ),
    ] {
        let label = texts.get(label);
        let rect = Rect::from_min_size(
            egui::pos2(x, control.top()),
            egui::vec2(key::width_for(cx, label, 72.0), tokens.size.button_h),
        );
        x = rect.right() + gap;
        key::key(
            cx,
            rect,
            id(key_id),
            KeySpec::text(label).enabled(enabled),
            Press::Click(action),
        );
    }

    // Its input, from the inputs of the system.
    top += 44.0;
    let control = setting(cx, page, top, "settings.midi_input");
    let inputs: Vec<Choice<'_>> = manager
        .inputs
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let current_input = controller.and_then(|controller| {
        manager
            .inputs
            .iter()
            .position(|input| *input == controller.input)
    });
    let input_names = manager.inputs.clone();
    let chosen = index.unwrap_or(0);
    let input_rect = Rect::from_min_size(control.min, egui::vec2(240.0, tokens.size.button_h));
    choice::select(
        cx,
        input_rect,
        id("settings-midi-input"),
        SelectSpec {
            name: texts.get("settings.midi_input"),
            choices: &inputs,
            current: current_input,
            enabled: controller.is_some() && !inputs.is_empty(),
        },
        move |input| {
            AppCommand::MidiInputSet {
                index: chosen,
                input: input_names.get(input).cloned().unwrap_or_default(),
            }
            .into()
        },
    );
    let state = match controller {
        Some(controller) if controller.open => texts.get("settings.midi_open").to_owned(),
        Some(controller) => controller
            .problem
            .clone()
            .unwrap_or_else(|| texts.get("settings.midi_closed").to_owned()),
        None => String::new(),
    };
    help(cx, input_rect, &state);

    // Its mapping: the file, Import from the folder of the explorer, Export into it.
    top += 56.0;
    let control = setting(cx, page, top, "settings.midi_mapping");
    let file = controller.map_or("", |controller| controller.mapping.as_str());
    let file_rect = Rect::from_min_size(control.min, egui::vec2(240.0, tokens.size.button_h));
    paint::text_elided(
        cx.painter(),
        theme,
        file_rect,
        file,
        tokens.text.body,
        color.text_primary,
    );
    let names: Vec<String> = manager
        .importable
        .iter()
        .map(|path| {
            path.file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
        })
        .collect();
    let imports: Vec<Choice<'_>> = names
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let import_rect = Rect::from_min_size(
        egui::pos2(file_rect.right() + gap, control.top()),
        egui::vec2(160.0, tokens.size.button_h),
    );
    let importable = manager.importable.clone();
    choice::select(
        cx,
        import_rect,
        id("settings-midi-import"),
        SelectSpec {
            name: texts.get("settings.midi_import"),
            choices: &imports,
            current: None,
            enabled: controller.is_some() && !imports.is_empty(),
        },
        move |file| {
            AppCommand::MidiMappingImport {
                index: chosen,
                path: importable.get(file).cloned().unwrap_or_default(),
            }
            .into()
        },
    );
    let export = texts.get("settings.midi_export");
    let export_rect = Rect::from_min_size(
        egui::pos2(import_rect.right() + gap, control.top()),
        egui::vec2(key::width_for(cx, export, 72.0), tokens.size.button_h),
    );
    let folder = view.browser.explorer.clone();
    key::key(
        cx,
        export_rect,
        id("settings-midi-export"),
        KeySpec::text(export).enabled(controller.is_some() && folder.is_some()),
        Press::Click(
            AppCommand::MidiMappingExport {
                index: chosen,
                folder: folder.unwrap_or_default(),
            }
            .into(),
        ),
    );
    help(cx, file_rect, texts.get("settings.midi_mapping_help"));

    // The assignments of the mapping.
    let top = section(cx, page, top + 60.0, "settings.midi_assignments");
    let left = page.left() + LABEL_X;
    let right = page.right() - LABEL_X;
    let columns = [
        ("settings.midi_function", 0.0),
        ("settings.midi_assigned", 0.34),
        ("settings.midi_type", 0.48),
        ("settings.midi_modifier", 0.64),
        ("settings.midi_message", 0.8),
    ];
    let width = right - left;
    let column_x = |share: f32| left + share * width;
    let header_top = page.top() + top;
    for (name, share) in columns {
        paint::text(
            cx.painter(),
            theme,
            Rect::from_min_size(
                egui::pos2(column_x(share), header_top),
                egui::vec2(width * 0.15, 18.0),
            ),
            Align2::LEFT_CENTER,
            texts.get(name),
            tokens.text.label,
            color.text_secondary,
        );
    }
    let bindings = controller.map_or(&[][..], |controller| controller.bindings.as_slice());
    let table = Rect::from_min_size(
        egui::pos2(left, header_top + 22.0),
        egui::vec2(width, BINDING_ROW * BINDING_ROWS as f32),
    );
    cx.painter()
        .rect_filled(table, paint::radius(tokens.radius.sm), color.bg_app);
    // The table is one control, as the list of the browser: a click picks the row under it.
    let shown = bindings.len().min(BINDING_ROWS);
    let rows_rect = Rect::from_min_size(table.min, egui::vec2(width, BINDING_ROW * shown as f32));
    let enabled = shown > 0;
    cx.record(
        AppCommand::MidiBindingSelect { index: 0 }.id(),
        rows_rect,
        ControlKind::Area,
        enabled,
    );
    let response = cx.ui.interact(
        rows_rect,
        id("settings-midi-bindings"),
        cx.sense(enabled, Sense::click()),
    );
    response.widget_info(|| {
        WidgetInfo::labeled(Role::List, enabled, texts.get("settings.midi_assignments"))
    });
    if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let row = ((pointer.y - rows_rect.top()) / BINDING_ROW).floor();
        if row >= 0.0 {
            cx.out.emit(AppCommand::MidiBindingSelect {
                index: row as usize,
            });
        }
    }
    let target_letter = manager.target.letter().to_string();
    for (row_index, binding) in bindings.iter().take(BINDING_ROWS).enumerate() {
        let row = Rect::from_min_size(
            egui::pos2(left, table.top() + BINDING_ROW * row_index as f32),
            egui::vec2(width, BINDING_ROW),
        );
        let painter = cx.painter();
        if manager.binding == Some(row_index) {
            painter.rect_filled(
                row.shrink(1.0),
                paint::radius(tokens.radius.sm),
                color.surface_selected,
            );
        }
        let deck_letter = binding.deck.map(|deck| deck.letter().to_string());
        let function = match &binding.command {
            Some(command) => texts.format(
                &format!("command.{command}"),
                &[("deck", deck_letter.as_deref().unwrap_or(&target_letter))],
            ),
            None => texts.get("settings.midi_modifier_key").to_owned(),
        };
        let assigned = match binding.deck {
            Some(deck) => texts.format(
                "settings.midi_deck",
                &[("deck", &deck.letter().to_string())],
            ),
            None => texts.get("settings.midi_global").to_owned(),
        };
        let kind = texts.get(match binding.control {
            ControlType::Button => "settings.midi_button",
            ControlType::ButtonHeld => "settings.midi_button_held",
            ControlType::Fader => "settings.midi_fader",
            ControlType::Encoder => "settings.midi_encoder",
        });
        let modifier = match binding.sets {
            Some((modifier, value)) => texts.format(
                "settings.midi_sets",
                &[
                    ("modifier", &modifier.to_string()),
                    ("value", &value.to_string()),
                ],
            ),
            None => binding
                .when
                .iter()
                .map(|(modifier, value)| format!("M{modifier} = {value}"))
                .collect::<Vec<_>>()
                .join(", "),
        };
        let message = binding.midi.as_deref().map_or_else(
            || texts.get("settings.midi_unassigned").to_owned(),
            |midi| midi_text(texts, midi),
        );
        let cells = [
            (function, columns[0].1, columns[1].1),
            (assigned, columns[1].1, columns[2].1),
            (kind.to_owned(), columns[2].1, columns[3].1),
            (modifier, columns[3].1, columns[4].1),
            (message, columns[4].1, 1.0),
        ];
        for (text, from, to) in cells {
            let cell = Rect::from_min_max(
                egui::pos2(column_x(from) + 4.0, row.top()),
                egui::pos2(column_x(to) - 8.0, row.bottom()),
            );
            paint::text_elided(
                painter,
                theme,
                cell,
                &text,
                tokens.text.body,
                color.text_primary,
            );
        }
    }
    if response.has_focus() {
        paint::focus_ring(cx.painter(), theme, rows_rect, tokens.radius.sm);
    }

    // Under the table: the deck of a new assignment, Add, Learn, Duplicate and Delete.
    let row_top = table.bottom() + gap;
    let decks = DeckId::ALL.map(|deck| deck.letter().to_string());
    let deck_choices: Vec<Choice<'_>> = decks
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        Rect::from_min_size(
            egui::pos2(left, row_top),
            egui::vec2(0.0, tokens.size.button_h),
        ),
        id("settings-midi-target"),
        &deck_choices,
        DeckId::ALL.iter().position(|deck| *deck == manager.target),
        |index| {
            Action::App(AppCommand::MidiTargetSet {
                deck: DeckId::ALL.get(index).copied().unwrap_or(DeckId::A),
            })
        },
    );
    let functions = midi_functions();
    let function_labels: Vec<String> = functions
        .iter()
        .map(|spec| texts.format(&format!("command.{}", spec.id), &[("deck", &target_letter)]))
        .collect();
    let function_choices: Vec<Choice<'_>> = function_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let add_rect = Rect::from_min_size(
        egui::pos2(drawn.right() + gap, row_top),
        egui::vec2(240.0, tokens.size.button_h),
    );
    choice::select(
        cx,
        add_rect,
        id("settings-midi-function"),
        SelectSpec {
            name: texts.get("settings.midi_add_function"),
            choices: &function_choices,
            current: None,
            enabled: controller.is_some(),
        },
        move |index| {
            AppCommand::MidiBindingAdd {
                command: functions
                    .get(index)
                    .map_or_else(String::new, |spec| spec.id.to_owned()),
            }
            .into()
        },
    );
    let selected = manager
        .binding
        .filter(|index| *index < bindings.len())
        .is_some();
    let mut x = add_rect.right() + gap;
    for (key_id, label, action, enabled, lit) in [
        (
            "settings-midi-learn",
            "settings.midi_learn",
            AppCommand::MidiLearnToggle,
            controller.is_some(),
            manager.learning,
        ),
        (
            "settings-midi-duplicate",
            "settings.midi_duplicate",
            AppCommand::MidiBindingDuplicate,
            selected,
            false,
        ),
        (
            "settings-midi-delete",
            "settings.midi_delete",
            AppCommand::MidiBindingDelete,
            selected,
            false,
        ),
    ] {
        let label = texts.get(label);
        let rect = Rect::from_min_size(
            egui::pos2(x, row_top),
            egui::vec2(key::width_for(cx, label, 72.0), tokens.size.button_h),
        );
        x = rect.right() + gap;
        key::key(
            cx,
            rect,
            id(key_id),
            KeySpec::text(label)
                .enabled(enabled)
                .lit(lit.then_some(color.text_primary)),
            Press::Click(Action::App(action)),
        );
    }
    let hint = if manager.learning {
        texts.get("settings.midi_learning")
    } else {
        texts.get("settings.midi_saved")
    };
    help(
        cx,
        Rect::from_min_max(
            egui::pos2(left, row_top),
            egui::pos2(right, row_top + tokens.size.button_h),
        ),
        hint,
    );
}

/// The commands a control can drive from the controller manager: those whose parameters
/// are the deck alone, as buttons, and those with one number besides, as faders.
pub fn midi_functions() -> Vec<&'static CommandSpec> {
    SPECS
        .iter()
        .filter(|spec| {
            let numbers = spec
                .params
                .iter()
                .filter(|param| matches!(param.kind, ParamKind::Number { .. }))
                .count();
            let others = spec
                .params
                .iter()
                .filter(|param| !matches!(param.kind, ParamKind::Number { .. } | ParamKind::Deck))
                .count();
            others == 0 && numbers <= 1
        })
        .collect()
}

/// A control as the table names it: `Note C2, ch 1`, `CC 14, ch 1` or `Pitch, ch 1`, from the
/// way the mapping writes it (`note 1 36`). Middle C is C4.
fn midi_text(texts: &Texts, midi: &str) -> String {
    const NOTES: [&str; 12] = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ];
    let parts: Vec<&str> = midi.split_whitespace().collect();
    let number = |text: &str| text.parse::<u8>().ok();
    match parts.as_slice() {
        ["note", channel, note] => match number(note) {
            Some(note) => texts.format(
                "settings.midi_note",
                &[
                    (
                        "note",
                        &format!(
                            "{}{}",
                            NOTES[usize::from(note % 12)],
                            i16::from(note / 12) - 1
                        ),
                    ),
                    ("channel", channel),
                ],
            ),
            None => midi.to_owned(),
        },
        ["cc", channel, controller] => texts.format(
            "settings.midi_cc",
            &[("number", controller), ("channel", channel)],
        ),
        ["pitch", channel] => texts.format("settings.midi_pitch", &[("channel", channel)]),
        _ => midi.to_owned(),
    }
}

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
            action: Command::MixerAutogainToggle.into(),
        },
    );

    // The models of the equaliser and of the filter: one help line serves both rows.
    let control = setting(cx, page, top + 100.0, "settings.eq_model");
    let eq_labels = EqModel::ALL.map(|model| {
        texts.get(match model {
            EqModel::Smooth => "settings.eq_smooth",
            EqModel::Soft => "settings.eq_soft",
            EqModel::Isolator => "settings.eq_isolator",
            EqModel::Steep => "settings.eq_steep",
        })
    });
    let eq_choices: Vec<Choice<'_>> = eq_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    choice::segmented(
        cx,
        control,
        id("settings-eq-model"),
        &eq_choices,
        EqModel::ALL
            .iter()
            .position(|model| *model == mixer.eq_model),
        |index| {
            Action::Engine(Command::MixerEqModelSet {
                model: EqModel::ALL[index],
            })
        },
    );
    let control = setting(cx, page, top + 140.0, "settings.filter_model");
    let filter_labels = FilterModel::ALL.map(|model| {
        texts.get(match model {
            FilterModel::Gentle => "settings.filter_gentle",
            FilterModel::Steep => "settings.filter_steep",
            FilterModel::Ladder => "settings.filter_ladder",
        })
    });
    let filter_choices: Vec<Choice<'_>> = filter_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-filter-model"),
        &filter_choices,
        FilterModel::ALL
            .iter()
            .position(|model| *model == mixer.filter_model),
        |index| {
            Action::Engine(Command::MixerFilterModelSet {
                model: FilterModel::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.models_help"),
    );

    effects_section(cx, page, 265.0, view);

    let top = section(cx, page, 590.0, "settings.external_section");
    switch(
        cx,
        page,
        top,
        Switch {
            name: "settings.external_eq_gain",
            help: "settings.external_help",
            on: mixer.external_eq_gain,
            action: Command::MixerExternalEqGainToggle.into(),
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
            action: Command::MixerExternalMasterVolumeToggle.into(),
        },
    );

    let top = section(cx, page, 745.0, "settings.crossfader_section");
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

    // The curve of the crossfader, which M2 had on the General page.
    let control = setting(cx, page, top + 50.0, "settings.crossfader_curve");
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
    switch(
        cx,
        page,
        top + 100.0,
        Switch {
            name: "settings.fade_markers",
            help: "settings.fade_markers_help",
            on: view.engine.fade_markers,
            action: Command::GlobalFadeMarkersToggle.into(),
        },
    );
}

/// The effect units: how many the effect row shows, and the routing of each one
/// (`docs/behavior/effects.md`, *Units and modes* and *Assignment and routing*).
fn effects_section(cx: &mut Cx<'_>, page: Rect, at: f32, view: &View<'_>) {
    let texts = cx.texts;
    let top = section(cx, page, at, "settings.effects_section");
    let control = setting(cx, page, top, "settings.effect_units");
    let unit_labels = EffectUnits::ALL.map(|units| {
        texts.get(match units {
            EffectUnits::Two => "settings.units_two",
            EffectUnits::Four => "settings.units_four",
        })
    });
    let unit_choices: Vec<Choice<'_>> = unit_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-effect-units"),
        &unit_choices,
        EffectUnits::ALL
            .iter()
            .position(|units| *units == view.settings.effect_units),
        |index| {
            Action::App(AppCommand::EffectUnitsSet {
                units: EffectUnits::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.effect_units_help"),
    );

    // Post fader has no meaning while the hardware mixer has the faders.
    let external = view.engine.mixer.mode == MixerMode::External;
    let routing_labels = FxRouting::ALL.map(|routing| {
        texts.get(match routing {
            FxRouting::Insert => "settings.routing_insert",
            FxRouting::PostFader => "settings.routing_post_fader",
        })
    });
    let routing_choices: Vec<Choice<'_>> = FxRouting::ALL
        .iter()
        .zip(&routing_labels)
        .map(|(routing, label)| Choice {
            label,
            enabled: !external || *routing == FxRouting::Insert,
        })
        .collect();
    let mut row_top = top + 50.0;
    let mut last = Rect::NOTHING;
    for unit in FxUnit::ALL {
        let number = unit.number().to_string();
        let name = texts.format("settings.unit_routing", &[("unit", &number)]);
        let control = setting_named(cx, page, row_top, &name);
        let current = view.engine.effects.unit(unit).routing;
        let drawn = choice::segmented(
            cx,
            control,
            id(("settings-routing", unit.index())),
            &routing_choices,
            FxRouting::ALL
                .iter()
                .position(|routing| *routing == current),
            |index| {
                Action::Engine(Command::FxUnitRoutingSet {
                    unit,
                    routing: FxRouting::ALL[index],
                })
            },
        );
        last = Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom()));
        row_top += 40.0;
    }
    help(cx, last, texts.get("settings.fx_routing_help"));
    switch(
        cx,
        page,
        row_top + 10.0,
        Switch {
            name: "settings.keep_values",
            help: "settings.keep_values_help",
            on: view.engine.effects.keep_values,
            action: Command::FxKeepValuesToggle.into(),
        },
    );
}

/// The minimum play times the Library page offers, in seconds.
const PLAYTIMES: [u32; 7] = [5, 10, 20, 30, 60, 120, 300];

fn library_page(cx: &mut Cx<'_>, page: Rect, view: &View<'_>) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let settings = &view.settings;

    let top = section(cx, page, 20.0, "settings.played_section");
    let control = setting(cx, page, top, "settings.min_playtime");
    let labels = PLAYTIMES.map(|seconds| seconds.to_string());
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-min-playtime"),
        &choices,
        PLAYTIMES
            .iter()
            .position(|seconds| *seconds == settings.min_playtime),
        |index| {
            Action::App(AppCommand::MinPlaytimeSet {
                seconds: PLAYTIMES[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.min_playtime_help"),
    );

    let top = section(cx, page, 125.0, "settings.collection_section");
    let control = setting(cx, page, top, "settings.library_file");
    // Not a control: a line on the row of its name, in the place of one.
    let theme = cx.theme;
    let line = Rect::from_min_size(
        control.min,
        egui::vec2(control.width(), theme.tokens.size.button_h),
    );
    paint::text_elided(
        cx.painter(),
        theme,
        line,
        &texts.format(
            "settings.library_file_help",
            &[("file", &settings.library_file)],
        ),
        theme.tokens.text.body,
        theme.tokens.color.text_secondary,
    );
    let control = setting(cx, page, top + 44.0, "settings.maintenance");
    let label = texts.get("settings.maintenance_open");
    let rect = Rect::from_min_size(
        control.min,
        egui::vec2(key::width_for(cx, label, 120.0), theme.tokens.size.button_h),
    );
    key::key(
        cx,
        rect,
        id("settings-maintenance"),
        KeySpec::text(label),
        Press::Click(AppCommand::LibraryMaintenanceOpen.into()),
    );
    help(
        cx,
        Rect::from_min_max(rect.min, egui::pos2(control.right(), rect.bottom())),
        texts.get("settings.maintenance_help"),
    );

    switch(
        cx,
        page,
        top + 88.0,
        Switch {
            name: "settings.check_at_start",
            help: "settings.check_at_start_help",
            on: view.settings.check_at_start,
            action: AppCommand::CheckAtStartToggle.into(),
        },
    );

    let top = section(cx, page, 312.0, "settings.files_section");
    let control = setting(cx, page, top, "settings.tag_writing");
    let mode = view.settings.tag_writing;
    let labels = TagWriting::ALL.map(|choice| {
        texts.get(match choice {
            TagWriting::Everything => "settings.tags_everything",
            TagWriting::PlatterOnly => "settings.tags_platter",
            TagWriting::Nothing => "settings.tags_nothing",
        })
    });
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let drawn = choice::segmented(
        cx,
        control,
        id("settings-tag-writing"),
        &choices,
        TagWriting::ALL.iter().position(|choice| *choice == mode),
        |index| {
            Action::App(AppCommand::TagWritingSet {
                mode: TagWriting::ALL[index],
            })
        },
    );
    help(
        cx,
        Rect::from_min_max(drawn.min, egui::pos2(control.right(), drawn.bottom())),
        texts.get("settings.tag_writing_help"),
    );

    // The music folders: one shown at a time, Add takes the folder the explorer shows.
    let control = setting(cx, page, top + 50.0, "settings.music_folders");
    let folders = &view.settings.music_folders;
    let shown = view
        .settings
        .music_folder
        .filter(|index| *index < folders.len())
        .or((!folders.is_empty()).then_some(0));
    let select = Rect::from_min_size(control.min, egui::vec2(260.0, tokens.size.button_h));
    let explorer = view.browser.explorer.clone();
    let mut x = select.right() + tokens.space.s3;
    for (key_id, label, action, enabled) in [
        (
            "settings-music-folder-add",
            "settings.music_folder_add",
            AppCommand::MusicFolderAdd {
                path: explorer.clone().unwrap_or_default(),
            },
            explorer
                .as_ref()
                .is_some_and(|folder| !folders.contains(folder)),
        ),
        (
            "settings-music-folder-remove",
            "settings.music_folder_remove",
            AppCommand::MusicFolderRemove {
                index: shown.unwrap_or(0),
            },
            shown.is_some(),
        ),
    ] {
        let label = texts.get(label);
        let rect = Rect::from_min_size(
            egui::pos2(x, control.top()),
            egui::vec2(key::width_for(cx, label, 72.0), tokens.size.button_h),
        );
        x = rect.right() + tokens.space.s3;
        key::key(
            cx,
            rect,
            id(key_id),
            KeySpec::text(label).enabled(enabled),
            Press::Click(Action::App(action)),
        );
    }
    let explorer_name = explorer
        .as_ref()
        .map_or_else(String::new, |folder| folder.display().to_string());
    help(
        cx,
        control,
        &texts.format(
            "settings.music_folders_help",
            &[("folder", explorer_name.as_str())],
        ),
    );
    switch(
        cx,
        page,
        top + 100.0,
        Switch {
            name: "settings.import_at_start",
            help: "settings.import_at_start_help",
            on: view.settings.import_at_start,
            action: AppCommand::ImportAtStartToggle.into(),
        },
    );
    // The list last, so its open list lies over the switch below it.
    let labels: Vec<String> = folders
        .iter()
        .map(|folder| folder.display().to_string())
        .collect();
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    choice::select(
        cx,
        select,
        id("settings-music-folder"),
        SelectSpec {
            name: texts.get("settings.music_folders"),
            choices: &choices,
            current: shown,
            enabled: !choices.is_empty(),
        },
        |index| AppCommand::MusicFolderSelect { index }.into(),
    );
}

/// A setting that is on or off: a key that toggles it, lit while it is on.
struct Switch {
    name: &'static str,
    help: &'static str,
    on: bool,
    action: Action,
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
        Press::Click(switch.action),
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
