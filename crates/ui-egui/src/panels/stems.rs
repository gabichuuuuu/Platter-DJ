//! The four stem lanes of a stem deck in the four-deck layout, in the place of its waveform
//! and stripe: for each stem its colour and name, the mute key, the volume bar, the effect
//! send key and its scrolling waveform.
//!
//! Placement: the stems deck of `docs/design/screens/M4-effects.html`. Behaviour note:
//! `docs/behavior/stems.md`.

use egui::{CornerRadius, Id, Key, Rect};
use platter_core::{DeckId, Stem};
use platter_engine::{Command, DeckSnapshot, StemSettings};

use crate::cx::{Cx, id};
use crate::paint;
use crate::panels::waveform;
use crate::widgets::key::{self, KeySpec, Press};
use crate::widgets::menu::{self, Item};
use crate::widgets::value::{self, Steps, ValueSpec};

const LANE_GAP: f32 = 2.0;
const NAME_WIDTH: f32 = 44.0;
const SWATCH: f32 = 6.0;
const MUTE_WIDTH: f32 = 20.0;
const VOLUME_WIDTH: f32 = 44.0;
const SEND_WIDTH: f32 = 24.0;
const KEY_HEIGHT: f32 = 18.0;
const GAP: f32 = 6.0;
/// Positions of the filter the menu of a lane offers, with the key of their text.
const FILTER_STEPS: [(f64, &str); 5] = [
    (-1.0, "stems.filter_low_full"),
    (-0.5, "stems.filter_low_half"),
    (0.0, "stems.filter_middle"),
    (0.5, "stems.filter_high_half"),
    (1.0, "stems.filter_high_full"),
];
const SEND_STEPS: [f64; 5] = [1.0, 0.75, 0.5, 0.25, 0.0];
/// The keys of the names of the stems, in the order of [`Stem::ALL`].
const NAMES: [&str; Stem::COUNT] = ["stems.drums", "stems.bass", "stems.other", "stems.vocals"];

/// The width of the column of mute keys beside the waveform of a stem deck in the parallel
/// lanes.
pub const MUTE_COLUMN_WIDTH: f32 = 56.0;
const STRIP: f32 = 3.0;

/// The four mute keys of the stems of `deck`, stacked in `rect`: each with a strip of the
/// colour of its stem and its name, lit in that colour while the stem is muted.
pub fn mute_column(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, snapshot: &DeckSnapshot) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let texts = cx.texts;
    let count = Stem::COUNT as f32;
    let height = KEY_HEIGHT * count + LANE_GAP * (count - 1.0);
    let top = rect.center().y - height / 2.0;
    for stem in Stem::ALL {
        let ink = tokens.stem[stem.index()];
        let settings = snapshot.controls.stems[stem.index()];
        let name = texts.get(NAMES[stem.index()]);
        let key_rect = Rect::from_min_size(
            egui::pos2(
                rect.left(),
                top + stem.index() as f32 * (KEY_HEIGHT + LANE_GAP),
            ),
            egui::vec2(rect.width(), KEY_HEIGHT),
        );
        let mute_name = texts.format("stems.mute_name", &[("stem", name)]);
        let mut spec = KeySpec::text(name).lit(settings.muted.then_some(ink));
        spec.name = &mute_name;
        spec.text = Some(tokens.text.label);
        key::key(
            cx,
            key_rect,
            id(("stem-mute", deck.index(), stem.index())),
            spec,
            Press::Click(Command::DeckStemMuteToggle { deck, stem }.into()),
        );
        let strip = Rect::from_min_size(key_rect.min, egui::vec2(STRIP, KEY_HEIGHT));
        cx.painter().rect_filled(strip, CornerRadius::ZERO, ink);
    }
}

/// The four lanes of the stems of `deck` in `rect`.
pub fn lanes(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, snapshot: &DeckSnapshot, zoom: usize) {
    let height = (rect.height() - LANE_GAP * (Stem::COUNT as f32 - 1.0)) / Stem::COUNT as f32;
    for stem in Stem::ALL {
        let top = rect.top() + stem.index() as f32 * (height + LANE_GAP);
        let lane = Rect::from_min_size(
            egui::pos2(rect.left(), top),
            egui::vec2(rect.width(), height),
        );
        lane_row(cx, lane, deck, stem, snapshot, zoom);
    }
}

fn lane_row(
    cx: &mut Cx<'_>,
    rect: Rect,
    deck: DeckId,
    stem: Stem,
    snapshot: &DeckSnapshot,
    zoom: usize,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let texts = cx.texts;
    let ink = tokens.stem[stem.index()];
    let settings = snapshot.controls.stems[stem.index()];
    let name = texts.get(NAMES[stem.index()]);
    let middle = rect.center().y;
    let mut x = rect.left();
    let mut cell = |width: f32, height: f32| {
        let cell = Rect::from_min_size(
            egui::pos2(x, middle - height / 2.0),
            egui::vec2(width, height),
        );
        x += width + GAP;
        cell
    };
    let name_cell = cell(NAME_WIDTH, rect.height());
    let mute = cell(MUTE_WIDTH, KEY_HEIGHT);
    let volume = cell(VOLUME_WIDTH, rect.height());
    let send = cell(SEND_WIDTH, KEY_HEIGHT);
    let wave = Rect::from_min_max(egui::pos2(x, rect.top()), rect.max);

    // The colour of the stem, always with its name.
    let swatch = Rect::from_min_size(
        egui::pos2(name_cell.left(), middle - SWATCH / 2.0),
        egui::vec2(SWATCH, SWATCH),
    );
    cx.painter().rect_filled(swatch, CornerRadius::ZERO, ink);
    paint::text_elided(
        cx.painter(),
        theme,
        Rect::from_min_max(
            egui::pos2(swatch.right() + tokens.space.s1, name_cell.top()),
            name_cell.max,
        ),
        name,
        tokens.text.label,
        color.text_primary,
    );

    let mute_name = texts.format("stems.mute_name", &[("stem", name)]);
    let mut spec = KeySpec::text(texts.get("stems.mute")).lit(settings.muted.then_some(ink));
    spec.name = &mute_name;
    spec.text = Some(tokens.text.label);
    key::key(
        cx,
        mute,
        id(("stem-mute", deck.index(), stem.index())),
        spec,
        Press::Click(Command::DeckStemMuteToggle { deck, stem }.into()),
    );

    volume_bar(cx, volume, deck, stem, settings, name);
    send_key(cx, send, deck, stem, settings, name);

    match snapshot.stem_peaks.as_deref() {
        Some(peaks) => waveform::stem_lane(cx, wave, snapshot, &peaks[stem.index()], ink, zoom),
        None => {
            cx.painter()
                .rect_filled(wave, CornerRadius::ZERO, color.bg_app);
        }
    }
}

/// The volume of a stem as a bar: the filled part is its level, dimmed while it is muted.
fn volume_bar(
    cx: &mut Cx<'_>,
    rect: Rect,
    deck: DeckId,
    stem: Stem,
    settings: StemSettings,
    name: &str,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let volume_name = cx.texts.format("stems.volume_name", &[("stem", name)]);
    let response = value::level_bar(
        cx,
        rect,
        id(("stem-volume", deck.index(), stem.index())),
        ValueSpec {
            name: &volume_name,
            position: f64::from(settings.volume),
            default: 1.0,
            steps: Steps::PERCENT,
            enabled: true,
        },
        |volume| Command::DeckStemVolumeSet { deck, stem, volume }.into(),
    );
    let bar = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 4.0));
    let painter = cx.painter();
    painter.rect(
        bar,
        CornerRadius::ZERO,
        color.bg_app,
        paint::hairline(theme, color.line_key),
        egui::StrokeKind::Inside,
    );
    let fill = Rect::from_min_size(
        bar.min + egui::vec2(1.0, 1.0),
        egui::vec2((bar.width() - 2.0) * settings.volume.clamp(0.0, 1.0), 2.0),
    );
    let ink = if settings.muted {
        color.text_secondary
    } else {
        color.text_primary
    };
    painter.rect_filled(fill, CornerRadius::ZERO, ink);
    if response.has_focus() {
        paint::focus_ring(painter, theme, bar.expand(3.0), tokens.radius.sm);
    }
}

/// The effect send key of a stem; its context menu sets the amount of the send and the
/// filter of the stem.
fn send_key(
    cx: &mut Cx<'_>,
    rect: Rect,
    deck: DeckId,
    stem: Stem,
    settings: StemSettings,
    name: &str,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let texts = cx.texts;
    let key_id = id(("stem-send", deck.index(), stem.index()));
    let send_name = texts.format("stems.send_name", &[("stem", name)]);
    let mut spec = KeySpec::text(texts.get("stems.send"))
        .lit(settings.send_on.then_some(tokens.color.text_primary));
    spec.name = &send_name;
    spec.text = Some(tokens.text.label);
    let response = key::key_response(
        cx,
        rect,
        key_id,
        spec,
        Press::Click(Command::DeckStemSendToggle { deck, stem }.into()),
    );
    let menu_id = key_id.with("menu");
    let keyed = response.has_focus()
        && cx
            .ui
            .input(|input| input.modifiers.shift && input.key_pressed(Key::F10));
    if keyed || response.secondary_clicked() {
        let at = response
            .interact_pointer_pos()
            .unwrap_or_else(|| rect.left_bottom());
        cx.ui.ctx().data_mut(|data| data.insert_temp(menu_id, at));
    }
    let Some(at) = cx
        .ui
        .ctx()
        .data(|data| data.get_temp::<egui::Pos2>(menu_id))
    else {
        return;
    };
    let percent = |amount: f64| format!("{:.0}", amount * 100.0);
    let send_labels: Vec<String> = SEND_STEPS
        .iter()
        .map(|amount| texts.format("stems.send_amount", &[("value", &percent(*amount))]))
        .collect();
    let filter_toggle = texts.get(if settings.filter_on {
        "stems.filter_off"
    } else {
        "stems.filter_on"
    });
    let mut items: Vec<Item<'_>> = SEND_STEPS
        .iter()
        .zip(&send_labels)
        .map(|(amount, label)| Item {
            label,
            action: Some(
                Command::DeckStemSendSet {
                    deck,
                    stem,
                    amount: *amount,
                }
                .into(),
            ),
        })
        .collect();
    items.push(Item {
        label: filter_toggle,
        action: Some(Command::DeckStemFilterToggle { deck, stem }.into()),
    });
    items.extend(FILTER_STEPS.iter().map(|(position, label)| {
        Item {
            label: texts.get(label),
            action: Some(
                Command::DeckStemFilterSet {
                    deck,
                    stem,
                    position: *position,
                }
                .into(),
            ),
        }
    }));
    let outcome = if cx.interactive {
        menu::menu(cx, menu_id.with("list"), at, &items, key_id)
    } else {
        menu::Outcome {
            open: false,
            picked: None,
        }
    };
    if !outcome.open {
        close(cx, menu_id);
    }
}

fn close(cx: &Cx<'_>, menu_id: Id) {
    cx.ui
        .ctx()
        .data_mut(|data| data.remove::<egui::Pos2>(menu_id));
}
