//! The face of a live deck: its header with the input it plays, the choice of that input, its
//! level, the type of the deck and a note on what it plays.
//!
//! Component: `LiveDeck` in `docs/design/DESIGN.md`. Behaviour note:
//! `docs/behavior/live-input.md`.

use egui::{Align2, CornerRadius, Rect, Role, Sense, WidgetInfo};
use platter_core::DeckId;
use platter_engine::{Command, DeckKind, InputSource, InputTarget};

use crate::command::Action;
use crate::cx::{Cx, id};
use crate::model::{InputState, View};
use crate::paint;
use crate::panels::remix::Frame;
use crate::panels::waveform::deck_color;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::meter::{self, MeterMemory, MeterSpec};

/// Inputs offered when the device does not say how many it has, as on the Input page.
const DEFAULT_INPUTS: u16 = 2;
const BADGE: f32 = 22.0;
const TITLE_HEIGHT: f32 = 18.0;
const TAG_WIDTH: f32 = 46.0;
const LABEL_WIDTH: f32 = 64.0;
const SELECT_WIDTH: f32 = 136.0;
const METER_HEIGHT: f32 = 10.0;
const NOTE_HEIGHT: f32 = 36.0;

/// Draws the live deck `deck` in `rect`.
pub fn deck(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>, frame: Frame) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let inner = rect.shrink(frame.padding);
    let letter = deck.letter().to_string();
    let target = InputTarget::Deck(deck);
    let route = view.engine.mixer.input_routes[target.index()];

    // The header: the badge, the input the deck plays and the tag.
    let painter = cx.painter();
    let badge = Rect::from_min_size(inner.min, egui::vec2(BADGE, BADGE));
    paint::badge(
        painter,
        theme,
        badge,
        deck.letter(),
        deck_color(theme, deck),
    );
    let (title, ink) = match route {
        Some(source) => (
            texts.format("settings.input_source", &[("inputs", source.name())]),
            color.text_primary,
        ),
        None => (texts.get("live.no_input").to_owned(), color.text_secondary),
    };
    let left = badge.right() + tokens.space.s4;
    let width = paint::text_width(painter, theme, &title, tokens.text.title)
        .min(inner.right() - left - TAG_WIDTH - tokens.space.s3);
    let title_rect = Rect::from_min_size(
        egui::pos2(left, inner.top()),
        egui::vec2(width, TITLE_HEIGHT),
    );
    paint::text_elided(painter, theme, title_rect, &title, tokens.text.title, ink);
    paint::tag(
        painter,
        theme,
        Rect::from_min_size(
            egui::pos2(title_rect.right() + tokens.space.s3, inner.top() + 1.0),
            egui::vec2(TAG_WIDTH, 16.0),
        ),
        texts.get("deck.kind_live"),
    );

    let row_height = tokens.size.button_h;
    let gap = tokens.space.s5;
    let top = inner.top() + frame.header + frame.padding;
    let label = |cx: &mut Cx<'_>, top: f32, key: &str| {
        paint::text(
            cx.painter(),
            theme,
            Rect::from_min_size(
                egui::pos2(inner.left(), top),
                egui::vec2(LABEL_WIDTH, row_height),
            ),
            Align2::LEFT_CENTER,
            texts.get(key),
            tokens.text.label,
            color.text_secondary,
        );
    };
    let control_left = inner.left() + LABEL_WIDTH + tokens.space.s3;

    // The input the deck plays.
    label(cx, top, "live.input");
    let select = Rect::from_min_size(
        egui::pos2(control_left, top),
        egui::vec2(SELECT_WIDTH, row_height),
    );
    let inputs = match view.audio.input {
        InputState::Running { channels } => channels,
        _ => DEFAULT_INPUTS,
    };
    let offered: Vec<InputSource> = InputSource::ALL
        .into_iter()
        .filter(|source| u16::from(source.last_input()) < inputs)
        .collect();

    // Its level, across the face.
    let meter_top = top + row_height + gap;
    label(
        cx,
        meter_top - (row_height - METER_HEIGHT) / 2.0,
        "live.level",
    );
    let meter_rect = Rect::from_min_max(
        egui::pos2(control_left, meter_top),
        egui::pos2(inner.right(), meter_top + METER_HEIGHT),
    );
    meter::meter(
        cx,
        meter_rect,
        &MeterSpec {
            levels: route.map_or([0.0; 2], |source| source.take(&view.engine.input.levels)),
            memory: MeterMemory::default(),
            segments: 24,
            vertical: false,
        },
    );
    let level_name = texts.format("live.level_name", &[("deck", &letter)]);
    let response = cx
        .ui
        .interact(meter_rect, id(("live-level", deck.index())), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::ProgressIndicator, true, &level_name));

    // The type of the deck.
    let type_top = meter_top + METER_HEIGHT + gap;
    label(cx, type_top, "live.type");
    let type_labels = [
        texts.get("settings.deck_track"),
        texts.get("settings.deck_live"),
        texts.get("settings.deck_remix"),
    ];
    let type_choices: Vec<Choice<'_>> = type_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    choice::segmented(
        cx,
        Rect::from_min_max(
            egui::pos2(control_left, type_top),
            egui::pos2(inner.right(), type_top + row_height),
        ),
        id(("live-type", deck.index())),
        &type_choices,
        Some(1),
        move |index| {
            Action::Engine(Command::DeckTypeSet {
                deck,
                kind: match index {
                    0 => DeckKind::Track,
                    2 => DeckKind::Remix,
                    _ => DeckKind::Live,
                },
            })
        },
    );

    let note = match view.audio.input {
        InputState::Running { .. } => texts.get("live.note"),
        _ => texts.get("live.note_no_device"),
    };
    let note_top = type_top + row_height + gap;
    paint::text_wrapped(
        cx.painter(),
        theme,
        Rect::from_min_size(
            egui::pos2(inner.left(), note_top),
            egui::vec2(inner.width(), NOTE_HEIGHT),
        ),
        note,
        tokens.text.body,
        color.text_secondary,
    );

    // The list last, so it opens over the rows below it.
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
    let current = match route {
        None => Some(0),
        Some(route) => offered
            .iter()
            .position(|source| *source == route)
            .map(|index| index + 1),
    };
    let input_name = texts.format("live.input_name", &[("deck", &letter)]);
    let offered = &offered;
    choice::select(
        cx,
        select,
        id(("live-input", deck.index())),
        SelectSpec {
            name: &input_name,
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
