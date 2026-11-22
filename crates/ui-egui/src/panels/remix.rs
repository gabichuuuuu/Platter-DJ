//! The face of a remix deck: its header, the quantize and the page keys, the four slots with
//! the cells of the page shown and the strip of each slot.
//!
//! Component: `RemixGrid` in `docs/design/DESIGN.md`. Behaviour note:
//! `docs/behavior/remix-deck.md`.

use std::path::PathBuf;

use egui::{
    Align2, Color32, CornerRadius, Id, Key, Pos2, Rect, Role, Sense, StrokeKind, WidgetInfo,
};
use platter_core::DeckId;
use platter_engine::{CellId, CellInfo, CellMode, Command, REMIX_QUANTIZE_BEATS, RemixSnapshot};

use crate::command::AppCommand;
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::model::View;
use crate::paint;
use crate::panels::waveform::{deck_bands, deck_color};
use crate::shell::ViewState;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::key::{self, KeySpec, Press};
use crate::widgets::menu::{self, Item};
use crate::widgets::value::{self, Steps, ValueSpec};

/// Cells of a slot a page shows.
pub const PAGE_CELLS: usize = 4;
/// Pages of a slot.
pub const PAGES: usize = 4;
const ROW_HEIGHT: f32 = 24.0;
const QUANTIZE_WIDTH: f32 = 56.0;
const PAGE_KEY: f32 = 24.0;
const KEY_HEIGHT: f32 = 18.0;
/// The keys of a slot over its volume bar.
const BAR_HEIGHT: f32 = 10.0;
const STRIP_HEIGHT: f32 = KEY_HEIGHT + 2.0 + BAR_HEIGHT;
const STRIP_KEY: f32 = 20.0;
const MAX_CELL_HEIGHT: f32 = 44.0;
const BADGE: f32 = 22.0;
const TITLE_HEIGHT: f32 = 18.0;
const TAG_WIDTH: f32 = 46.0;
const TEMPO_WIDTH: f32 = 72.0;

/// The size of the deck the face fills: its padding and the height of its header.
pub struct Frame {
    pub padding: f32,
    pub header: f32,
}

/// Draws the remix deck `deck` in `rect`.
pub fn deck(
    cx: &mut Cx<'_>,
    rect: Rect,
    deck: DeckId,
    view: &View<'_>,
    state: &ViewState,
    frame: Frame,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, tokens.color.surface_panel);
    let inner = rect.shrink(frame.padding);
    header(
        cx,
        Rect::from_min_size(inner.min, egui::vec2(inner.width(), frame.header)),
        deck,
        view,
    );
    let grid = Rect::from_min_max(
        egui::pos2(inner.left(), inner.top() + frame.header + frame.padding),
        inner.max,
    );
    let remix = &view.engine.decks[deck.index()].remix;
    let page = state.remix_page[deck.index()].min(PAGES - 1);
    let row = Rect::from_min_size(grid.min, egui::vec2(grid.width(), ROW_HEIGHT));
    deck_row(cx, row, deck, remix, page);
    let slots = Rect::from_min_max(
        egui::pos2(grid.left(), row.bottom() + tokens.space.s3),
        grid.max,
    );
    let gap = tokens.space.s2;
    let count = remix.cells.len();
    let width = (slots.width() - gap * (count as f32 - 1.0)) / count as f32;
    for slot in 0..count {
        let column = Rect::from_min_size(
            egui::pos2(slots.left() + slot as f32 * (width + gap), slots.top()),
            egui::vec2(width, slots.height()),
        );
        slot_column(cx, column, deck, remix, slot, page);
    }
}

/// The badge, the title with its tag and the tempo of the master clock the deck follows.
fn header(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let badge = Rect::from_min_size(rect.min, egui::vec2(BADGE, BADGE));
    paint::badge(
        painter,
        theme,
        badge,
        deck.letter(),
        deck_color(theme, deck),
    );
    let tempo = Rect::from_min_max(egui::pos2(rect.right() - TEMPO_WIDTH, rect.top()), rect.max);
    let left = badge.right() + tokens.space.s4;
    let title = texts.get("remix.title");
    let width = paint::text_width(painter, theme, title, tokens.text.title)
        .min(tempo.left() - left - TAG_WIDTH - 2.0 * tokens.space.s3);
    let title_rect = Rect::from_min_size(
        egui::pos2(left, rect.top()),
        egui::vec2(width, TITLE_HEIGHT),
    );
    paint::text_elided(
        painter,
        theme,
        title_rect,
        title,
        tokens.text.title,
        color.text_primary,
    );
    let tag = Rect::from_min_size(
        egui::pos2(title_rect.right() + tokens.space.s3, rect.top() + 1.0),
        egui::vec2(TAG_WIDTH, 16.0),
    );
    paint::tag(painter, theme, tag, texts.get("deck.kind_remix"));
    paint::text(
        painter,
        theme,
        Rect::from_min_size(tempo.min, egui::vec2(tempo.width(), 22.0)),
        Align2::RIGHT_CENTER,
        &format::bpm(view.engine.clock.bpm),
        tokens.text.tempo,
        color.text_primary,
    );
    paint::text(
        painter,
        theme,
        Rect::from_min_size(
            tempo.min + egui::vec2(0.0, 22.0),
            egui::vec2(tempo.width(), 14.0),
        ),
        Align2::RIGHT_CENTER,
        texts.get("remix.clock"),
        tokens.text.readout,
        color.text_secondary,
    );
}

/// `Quantize` with its list, then the four page keys at the right.
fn deck_row(cx: &mut Cx<'_>, row: Rect, deck: DeckId, remix: &RemixSnapshot, page: usize) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let label = texts.get("remix.quantize");
    let painter = cx.painter();
    let label_width = paint::text_width(painter, theme, label, tokens.text.label);
    paint::text(
        painter,
        theme,
        Rect::from_min_size(row.min, egui::vec2(label_width, row.height())),
        Align2::LEFT_CENTER,
        label,
        tokens.text.label,
        tokens.color.text_secondary,
    );
    let off = texts.get("remix.quantize_off");
    let numbers: Vec<String> = REMIX_QUANTIZE_BEATS.iter().map(u32::to_string).collect();
    let choices: Vec<Choice<'_>> = std::iter::once(off)
        .chain(numbers.iter().map(String::as_str))
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let current = match remix.quantize {
        None => Some(0),
        Some(beats) => REMIX_QUANTIZE_BEATS
            .iter()
            .position(|choice| *choice == beats)
            .map(|index| index + 1),
    };
    let letter = deck.letter().to_string();
    let name = texts.format("remix.quantize_name", &[("deck", &letter)]);
    choice::small_select(
        cx,
        Rect::from_min_size(
            egui::pos2(row.left() + label_width + tokens.space.s3, row.top()),
            egui::vec2(QUANTIZE_WIDTH, row.height()),
        ),
        id(("remix-quantize", deck.index())),
        SelectSpec {
            name: &name,
            choices: &choices,
            current,
            enabled: true,
        },
        move |index| {
            Command::RemixQuantizeSet {
                deck,
                beats: index
                    .checked_sub(1)
                    .and_then(|index| REMIX_QUANTIZE_BEATS.get(index).copied()),
            }
            .into()
        },
    );

    let gap = tokens.space.s1;
    let ink = deck_color(theme, deck);
    for shown in 0..PAGES {
        let left =
            row.right() - (PAGES - shown) as f32 * PAGE_KEY - (PAGES - 1 - shown) as f32 * gap;
        let rect = Rect::from_min_size(egui::pos2(left, row.top()), egui::vec2(PAGE_KEY, PAGE_KEY));
        let number = (shown + 1).to_string();
        let name = texts.format("remix.page_name", &[("page", &number), ("deck", &letter)]);
        let lit = if shown == page {
            Some(tokens.color.text_primary)
        } else {
            plays_on(remix, shown).then_some(ink)
        };
        let mut spec = KeySpec::text(&number).lit(lit);
        spec.name = &name;
        spec.text = Some(tokens.text.label);
        key::key(
            cx,
            rect,
            id(("remix-page", deck.index(), shown)),
            spec,
            Press::Click(AppCommand::RemixPageSet { deck, page: shown }.into()),
        );
    }
}

/// Whether a slot plays a cell of page `page`.
fn plays_on(remix: &RemixSnapshot, page: usize) -> bool {
    remix
        .playing
        .iter()
        .flatten()
        .any(|cell| cell / PAGE_CELLS == page)
}

/// The cells of the page shown of one slot, from the top, and the strip of the slot under
/// them.
fn slot_column(
    cx: &mut Cx<'_>,
    column: Rect,
    deck: DeckId,
    remix: &RemixSnapshot,
    slot: usize,
    page: usize,
) {
    let tokens = &cx.theme.tokens;
    let gap = tokens.space.s2;
    let room = column.height() - STRIP_HEIGHT - gap;
    let height =
        ((room - gap * (PAGE_CELLS as f32 - 1.0)) / PAGE_CELLS as f32).min(MAX_CELL_HEIGHT);
    for row in 0..PAGE_CELLS {
        let rect = Rect::from_min_size(
            egui::pos2(column.left(), column.top() + row as f32 * (height + gap)),
            egui::vec2(column.width(), height),
        );
        let Some(at) = CellId::new(slot, page * PAGE_CELLS + row) else {
            continue;
        };
        cell(cx, rect, deck, remix, at);
    }
    let strip_top = column.top() + PAGE_CELLS as f32 * (height + gap);
    strip(
        cx,
        Rect::from_min_size(
            egui::pos2(column.left(), strip_top),
            egui::vec2(column.width(), STRIP_HEIGHT),
        ),
        deck,
        remix,
        slot,
    );
}

/// How a cell key is held, between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CellHold {
    #[default]
    Up,
    /// Pressed: the trigger went out, the release goes out when it comes up.
    Pressed,
    /// Pressed with Shift: the slot was stopped, the release sends nothing.
    Stopped,
}

fn cell(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, remix: &RemixSnapshot, at: CellId) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let info = remix.cells[at.slot][at.cell].as_ref();
    let loading = remix.loading[at.slot][at.cell];
    let playing = remix.playing[at.slot] == Some(at.cell);
    let waiting = remix.waiting[at.slot] == Some(at.cell);
    let number = (at.cell + 1).to_string();
    let slot_number = (at.slot + 1).to_string();
    let name = match info {
        Some(info) => texts.format(
            "remix.cell_name",
            &[
                ("cell", &number),
                ("slot", &slot_number),
                ("name", &info.name),
            ],
        ),
        None => texts.format(
            "remix.empty_cell_name",
            &[("cell", &number), ("slot", &slot_number)],
        ),
    };
    let enabled = info.is_some();
    let key_id = id(("remix-cell", deck.index(), at.slot, at.cell));
    cx.record("remix.cell.trigger", rect, ControlKind::Hold, enabled);
    let response = cx
        .ui
        .interact(rect, key_id, cx.sense(enabled, Sense::click()));
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, &name));
    let focused = response.has_focus();
    if focused {
        cx.out.claim(&[Key::Space, Key::Enter]);
    }
    let (keyboard, shift) = cx.ui.input(|input| {
        let keys = input.key_down(Key::Space) || input.key_down(Key::Enter);
        (focused && keys, input.modifiers.shift)
    });
    let down = enabled && (response.is_pointer_button_down_on() || keyboard);
    let held = cx
        .ui
        .ctx()
        .data(|data| data.get_temp::<CellHold>(key_id))
        .unwrap_or_default();
    if down != (held != CellHold::Up) {
        let next = match (down, shift && (playing || waiting)) {
            (false, _) => CellHold::Up,
            (true, true) => CellHold::Stopped,
            (true, false) => CellHold::Pressed,
        };
        cx.ui.ctx().data_mut(|data| data.insert_temp(key_id, next));
        match (held, next) {
            (_, CellHold::Stopped) => cx.out.emit(Command::RemixSlotStop {
                deck,
                slot: at.slot,
            }),
            (_, CellHold::Pressed) => cx.out.emit(Command::RemixCellTrigger { deck, at }),
            (CellHold::Pressed, CellHold::Up) => {
                cx.out.emit(Command::RemixCellRelease { deck, at });
            }
            _ => {}
        }
    }
    drop_on_cell(cx, rect, deck, at, key_id);
    cell_menu(cx, &response, rect, deck, at, info, key_id);

    let painter = cx.painter();
    let corner = paint::radius(tokens.radius.lg);
    let hovered = response.hovered();
    let numeral = Rect::from_min_max(
        egui::pos2(rect.left() + tokens.space.s3, rect.top()),
        egui::pos2(rect.left() + tokens.space.s3 + 14.0, rect.bottom()),
    );
    match info {
        Some(info) => {
            let fill = if playing || down {
                color.surface_pressed
            } else if hovered {
                color.surface_hover
            } else {
                color.surface_control
            };
            painter.rect(
                rect,
                corner,
                fill,
                paint::hairline(theme, color.line_key),
                StrokeKind::Inside,
            );
            let led = if playing {
                Some(deck_color(theme, deck))
            } else if waiting {
                Some(deck_bands(theme, deck)[0])
            } else {
                None
            };
            if let Some(led) = led {
                let strip =
                    Rect::from_min_size(rect.min, egui::vec2(rect.width(), tokens.stroke.led));
                painter.rect_filled(
                    strip,
                    CornerRadius {
                        nw: corner.nw,
                        ne: corner.ne,
                        sw: 0,
                        se: 0,
                    },
                    led,
                );
            }
            let text = Rect::from_min_max(
                egui::pos2(
                    rect.left() + tokens.space.s3,
                    rect.top() + tokens.stroke.led,
                ),
                egui::pos2(rect.right() - tokens.space.s2, rect.bottom()),
            );
            let middle = text.center().y;
            paint::text_elided(
                painter,
                theme,
                Rect::from_min_max(text.min, egui::pos2(text.right(), middle)),
                &info.name,
                tokens.text.label,
                color.text_primary,
            );
            paint::text_elided(
                painter,
                theme,
                Rect::from_min_max(egui::pos2(text.left(), middle), text.max),
                &details(cx.texts, info),
                tokens.text.readout,
                color.text_secondary,
            );
        }
        None => {
            let fill = if hovered {
                color.surface_hover
            } else {
                Color32::TRANSPARENT
            };
            painter.rect(
                rect,
                corner,
                fill,
                paint::hairline(theme, color.line_key),
                StrokeKind::Inside,
            );
            paint::text(
                painter,
                theme,
                numeral,
                Align2::LEFT_CENTER,
                &number,
                tokens.text.label,
                color.text_secondary,
            );
            if loading {
                paint::text_elided(
                    painter,
                    theme,
                    Rect::from_min_max(
                        egui::pos2(numeral.right() + tokens.space.s2, rect.top()),
                        egui::pos2(rect.right() - tokens.space.s2, rect.bottom()),
                    ),
                    texts.get("deck.loading"),
                    tokens.text.label,
                    color.text_secondary,
                );
            }
        }
    }
    if focused {
        paint::focus_ring(painter, theme, rect, tokens.radius.lg);
    }
}

/// The second line of a cell: its mode, its tempo, and whether it is gated and reversed.
fn details(texts: &crate::i18n::Texts, info: &CellInfo) -> String {
    let mut parts = vec![
        texts
            .get(match info.mode {
                CellMode::Loop => "remix.loop",
                CellMode::OneShot => "remix.one_shot",
            })
            .to_owned(),
    ];
    if let Some(bpm) = info.bpm {
        parts.push(format::bpm(bpm.value()));
    }
    if info.gate {
        parts.push(texts.get("remix.gate").to_owned());
    }
    if info.reverse {
        parts.push(texts.get("remix.reverse_short").to_owned());
    }
    parts.join(" \u{b7} ")
}

/// A track dragged from the browser and released on the cell is loaded into it.
fn drop_on_cell(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, at: CellId, key_id: Id) {
    if !cx.interactive {
        return;
    }
    let theme = cx.theme;
    let drop = cx.ui.interact(rect, key_id.with("drop"), Sense::hover());
    if drop.dnd_hover_payload::<PathBuf>().is_some() {
        cx.painter().rect_stroke(
            rect,
            paint::radius(theme.tokens.radius.lg),
            paint::hairline(theme, theme.tokens.color.text_primary),
            StrokeKind::Inside,
        );
    }
    if let Some(path) = drop.dnd_release_payload::<PathBuf>() {
        cx.out.emit(Command::RemixCellLoad {
            deck,
            at,
            path: path.as_ref().clone(),
        });
    }
}

/// The context menu of a cell: its mode, latch or gate, reverse, the captures from the other
/// decks and Clear. An empty cell offers the captures only.
fn cell_menu(
    cx: &mut Cx<'_>,
    response: &egui::Response,
    rect: Rect,
    deck: DeckId,
    at: CellId,
    info: Option<&CellInfo>,
    key_id: Id,
) {
    let texts = cx.texts;
    let menu_id = key_id.with("menu");
    let keyed = response.has_focus()
        && cx
            .ui
            .input(|input| input.modifiers.shift && input.key_pressed(Key::F10));
    // An empty cell senses no click, so its secondary click is read from the pointer.
    let secondary = response.secondary_clicked()
        || (info.is_none()
            && cx.ui.input(|input| {
                input.pointer.secondary_clicked()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| rect.contains(pos))
            }));
    if keyed || secondary {
        let at = response
            .interact_pointer_pos()
            .or_else(|| cx.ui.input(|input| input.pointer.interact_pos()))
            .unwrap_or_else(|| rect.left_bottom());
        cx.ui.ctx().data_mut(|data| data.insert_temp(menu_id, at));
    }
    let Some(open_at) = cx.ui.ctx().data(|data| data.get_temp::<Pos2>(menu_id)) else {
        return;
    };
    let captures: Vec<(DeckId, String)> = DeckId::ALL
        .into_iter()
        .filter(|from| *from != deck)
        .map(|from| {
            let letter = from.letter().to_string();
            (from, texts.format("remix.capture", &[("deck", &letter)]))
        })
        .collect();
    let mut items: Vec<Item<'_>> = Vec::new();
    if let Some(info) = info {
        let (mode, mode_label) = match info.mode {
            CellMode::Loop => (CellMode::OneShot, "remix.make_one_shot"),
            CellMode::OneShot => (CellMode::Loop, "remix.make_loop"),
        };
        items.push(Item {
            label: texts.get(mode_label),
            action: Some(Command::RemixCellModeSet { deck, at, mode }.into()),
        });
        items.push(Item {
            label: texts.get(if info.gate {
                "remix.make_latch"
            } else {
                "remix.make_gate"
            }),
            action: Some(
                Command::RemixCellTriggerModeSet {
                    deck,
                    at,
                    gate: !info.gate,
                }
                .into(),
            ),
        });
        items.push(Item {
            label: texts.get(if info.reverse {
                "remix.forwards"
            } else {
                "remix.reverse"
            }),
            action: Some(Command::RemixCellReverseToggle { deck, at }.into()),
        });
    }
    items.extend(captures.iter().map(|(from, label)| {
        Item {
            label,
            action: Some(
                Command::RemixCellCapture {
                    deck,
                    at,
                    from: *from,
                }
                .into(),
            ),
        }
    }));
    if info.is_some() {
        items.push(Item {
            label: texts.get("remix.clear"),
            action: Some(Command::RemixCellClear { deck, at }.into()),
        });
    }
    let outcome = if cx.interactive {
        menu::menu(cx, menu_id.with("list"), open_at, &items, key_id)
    } else {
        menu::Outcome {
            open: false,
            picked: None,
        }
    };
    if !outcome.open {
        cx.ui.ctx().data_mut(|data| data.remove::<Pos2>(menu_id));
    }
}

/// Mute, punch and Stop, with the volume bar of the slot under them.
fn strip(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, remix: &RemixSnapshot, slot: usize) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let number = (slot + 1).to_string();
    let values = [("slot", number.as_str())];
    let gap = tokens.space.s1;
    let mut x = rect.left();
    let mut place = |width: f32| {
        let placed = Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, KEY_HEIGHT));
        x += width + gap;
        placed
    };

    let mute_name = texts.format("remix.mute_name", &values);
    let mut spec =
        KeySpec::text(texts.get("remix.mute")).lit(remix.muted[slot].then_some(color.text_primary));
    spec.name = &mute_name;
    spec.text = Some(tokens.text.label);
    key::key(
        cx,
        place(STRIP_KEY),
        id(("remix-mute", deck.index(), slot)),
        spec,
        Press::Click(Command::RemixSlotMuteToggle { deck, slot }.into()),
    );

    let punch_name = texts.format("remix.punch_name", &values);
    let mut spec = KeySpec::text(texts.get("remix.punch"))
        .lit(remix.punch[slot].then_some(color.text_primary));
    spec.name = &punch_name;
    spec.text = Some(tokens.text.label);
    key::key(
        cx,
        place(STRIP_KEY),
        id(("remix-punch", deck.index(), slot)),
        spec,
        Press::Click(Command::RemixSlotPunchToggle { deck, slot }.into()),
    );

    let stop = texts.get("remix.stop");
    let stop_name = texts.format("remix.stop_name", &values);
    let busy = remix.playing[slot].is_some() || remix.waiting[slot].is_some();
    let stop_width = key::width_for(cx, stop, STRIP_KEY);
    let mut spec = KeySpec::text(stop).enabled(busy);
    spec.name = &stop_name;
    spec.text = Some(tokens.text.label);
    key::key(
        cx,
        place(stop_width),
        id(("remix-stop", deck.index(), slot)),
        spec,
        Press::Click(Command::RemixSlotStop { deck, slot }.into()),
    );

    let bar_area = Rect::from_min_max(
        egui::pos2(rect.left(), rect.bottom() - BAR_HEIGHT),
        rect.max,
    );
    let volume_name = texts.format("remix.volume_name", &values);
    let volume = remix.volume[slot];
    let response = value::level_bar(
        cx,
        bar_area,
        id(("remix-volume", deck.index(), slot)),
        ValueSpec {
            name: &volume_name,
            position: volume,
            default: 1.0,
            steps: Steps::PERCENT,
            enabled: true,
        },
        move |level| Command::RemixSlotVolumeSet { deck, slot, level }.into(),
    );
    let bar = Rect::from_center_size(bar_area.center(), egui::vec2(bar_area.width(), 4.0));
    let painter = cx.painter();
    painter.rect(
        bar,
        CornerRadius::ZERO,
        color.bg_app,
        paint::hairline(theme, color.line_key),
        StrokeKind::Inside,
    );
    let fill = Rect::from_min_size(
        bar.min + egui::vec2(1.0, 1.0),
        egui::vec2((bar.width() - 2.0) * volume.clamp(0.0, 1.0) as f32, 2.0),
    );
    let ink = if remix.muted[slot] {
        color.text_secondary
    } else {
        color.text_primary
    };
    painter.rect_filled(fill, CornerRadius::ZERO, ink);
    if response.has_focus() {
        paint::focus_ring(painter, theme, bar.expand(3.0), tokens.radius.sm);
    }
}
