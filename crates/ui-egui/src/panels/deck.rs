//! A deck: its header, the overview of its track, the hotcues, the loop controls, a page of
//! the advanced panel, the transport keys and the tempo control.
//!
//! Components: `DeckHeader`, `TransportButton`, `TempoControl`, `HotcuePads`, `LoopControls`,
//! `MovePanel`, `CuePanel`, `GridEditor` and `Notice` in `docs/design/DESIGN.md`; placement from
//! `docs/design/screens/M2-main.html` and, for the loop row and the pages,
//! `docs/design/screens/First-version.html`. Behaviour notes: `docs/behavior/transport.md`,
//! `docs/behavior/beatgrid.md`, `docs/behavior/loops.md` and `docs/behavior/cues.md`.

use std::path::PathBuf;

use egui::{
    Align2, Color32, CornerRadius, Key, Rect, Role, Sense, Shape, Stroke, StrokeKind, WidgetInfo,
};
use platter_core::{Bpm, Cue, CueKind, DeckId, HotcueSlot};
use platter_engine::{
    BpmScale, Command, CueStep, DeckSnapshot, Direction, MoveMode, MoveSize, SizeStep,
};

use crate::command::{Action, AppCommand, DeckPage};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::i18n::Texts;
use crate::icons::Glyph;
use crate::model::{DeckNotice, View};
use crate::paint;
use crate::panels::waveform::{self, deck_color};
use crate::shell::ViewState;
use crate::tokens::{TextStyle, Tokens, Weight};
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::key::{self, Face, KeySpec, Press};
use crate::widgets::value::{self, FADER_WIDTH, FaderSpec, Steps, ValueSpec};

/// How much the bend keys change the speed while held.
pub const NUDGE_AMOUNT: f64 = 0.04;
/// Seconds left below which the remaining time turns to the warning colour.
const ENDING_SECONDS: f64 = 30.0;
/// Beats per minute the tempo keys and the arrows of the tempo field add or take away.
pub const BPM_STEP: f64 = 0.01;
/// Seconds the marker keys move a grid marker.
pub const MARKER_STEP_SECONDS: f64 = 0.005;
/// Where the tempo field starts when neither the grid nor the tag has a tempo.
const DEFAULT_FIELD_BPM: f64 = 120.0;
const GRID_ROW_HEIGHT: f32 = 24.0;
const GRID_KEY_WIDTH: f32 = 24.0;
const BPM_FIELD_WIDTH: f32 = 64.0;
const LOOP_ROW_HEIGHT: f32 = 28.0;
const ARROW_WIDTH: f32 = 28.0;
const LOOP_SIZE_WIDTH: f32 = 48.0;
const MOVE_SIZE_WIDTH: f32 = 40.0;
const PAD_HEIGHT: f32 = 38.0;
/// Height of the strip of its colour along the top of a pad that holds a cue.
const PAD_STRIP: f32 = 4.0;
const PAD_COLUMNS: usize = 4;
const CUE_SQUARE: f32 = 12.0;
const CUE_TYPE_WIDTH: f32 = 96.0;
const CUE_POSITION_WIDTH: f32 = 68.0;

/// The name a track is shown by: its title tag, or the name of its file.
fn title_of(snapshot: &DeckSnapshot) -> Option<String> {
    let track = snapshot.track.as_ref()?;
    track.tags.title.clone().or_else(|| {
        track
            .path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
    })
}

/// Whether the deck holds a track that can be played: one is loaded and no other is on its
/// way.
fn is_ready(snapshot: &DeckSnapshot) -> bool {
    snapshot.track.is_some() && !snapshot.loading
}

pub fn deck(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let snapshot = &view.engine.decks[deck.index()];
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let inner = rect.shrink(tokens.space.s4);

    // A track dragged from the file list and released here is loaded.
    let drop = cx
        .ui
        .interact(rect, id(("deck-drop", deck.index())), Sense::hover());
    if cx.interactive {
        if drop.dnd_hover_payload::<PathBuf>().is_some() {
            cx.painter().rect_stroke(
                rect,
                CornerRadius::ZERO,
                paint::hairline(theme, color.text_primary),
                StrokeKind::Inside,
            );
        }
        if let Some(path) = drop.dnd_release_payload::<PathBuf>() {
            cx.out.emit(Command::DeckLoad {
                deck,
                path: path.as_ref().clone(),
            });
        }
    }

    header(
        cx,
        Rect::from_min_size(inner.min, egui::vec2(inner.width(), 56.0)),
        deck,
        view,
        state,
    );
    let overview = Rect::from_min_size(
        inner.min + egui::vec2(0.0, 66.0),
        egui::vec2(inner.width(), 72.0),
    );
    waveform::overview(cx, overview, deck, snapshot);

    let controls = Rect::from_min_max(
        egui::pos2(inner.left(), overview.bottom() + 10.0),
        inner.max,
    );
    let tempo_column = Rect::from_min_max(
        egui::pos2(controls.right() - 64.0, controls.top()),
        controls.max,
    );
    let left = Rect::from_min_max(
        controls.min,
        egui::pos2(tempo_column.left() - tokens.space.s5, controls.bottom()),
    );
    let mapping = state.cue_map[deck.index()];
    // A notice takes the place of the hotcues for as long as it shows.
    if let Some(notice) = view.notices[deck.index()] {
        let area = Rect::from_min_size(left.min, egui::vec2(left.width(), 52.0));
        deck_notice(cx, area, deck, notice);
    } else {
        hotcue_pads(cx, left, deck, snapshot, mapping);
    }
    transport(cx, left, deck, snapshot);
    loop_controls(cx, left, deck, snapshot);
    let page = state.deck_page[deck.index()];
    match page {
        DeckPage::Move => move_page(cx, left, deck, snapshot),
        DeckPage::Cue => cue_page(cx, left, deck, snapshot, mapping),
        DeckPage::Grid => grid_page(cx, left, deck, snapshot),
    }
    page_selector(cx, left, deck, page);
    tempo(cx, tempo_column, deck, snapshot);
}

/// The tops of the two rows of a page of the advanced panel, above the transport keys at
/// the bottom of `area`.
fn page_rows(area: Rect, tokens: &Tokens) -> (f32, f32) {
    let second = area.bottom() - tokens.size.transport_h - tokens.space.s3 - GRID_ROW_HEIGHT;
    (second - tokens.space.s2 - GRID_ROW_HEIGHT, second)
}

/// One arrow of a stepper: its name, whether it has somewhere to step to, and what it sends.
struct Arrow<'a> {
    name: &'a str,
    enabled: bool,
    action: Action,
}

/// A reading between an arrow that steps it down and one that steps it up, along `row`.
fn stepper(
    cx: &mut Cx<'_>,
    row: &mut Row,
    salt: (&'static str, usize),
    reading: (&str, TextStyle, f32),
    arrows: [Arrow<'_>; 2],
) {
    let color = &cx.theme.tokens.color;
    let (text, style, width) = reading;
    let [down, up] = arrows;
    let active = down.enabled || up.enabled;
    let rect = row.place(ARROW_WIDTH);
    key::key(
        cx,
        rect,
        id((salt, 0)),
        KeySpec::icon(Glyph::ChevronLeft, 14.0, down.name).enabled(down.enabled),
        Press::Click(down.action),
    );
    let rect = row.place(width);
    let ink = if active {
        color.text_primary
    } else {
        color.text_disabled
    };
    paint::text(
        cx.painter(),
        cx.theme,
        rect,
        Align2::CENTER_CENTER,
        text,
        style,
        ink,
    );
    let rect = row.place(ARROW_WIDTH);
    key::key(
        cx,
        rect,
        id((salt, 1)),
        KeySpec::icon(Glyph::ChevronRight, 14.0, up.name).enabled(up.enabled),
        Press::Click(up.action),
    );
}

/// Whether one jump or move of the deck has a length: the fine steps always have, the
/// sizes in beats need a tempo, and the length of the loop needs the loop or a tempo.
fn step_has_length(snapshot: &DeckSnapshot) -> bool {
    let controls = &snapshot.controls;
    let has_tempo = snapshot.grid.bpm().is_some();
    controls.move_fine
        || match controls.move_size {
            MoveSize::Beats(_) => has_tempo,
            MoveSize::Loop => has_tempo || snapshot.looping.region.is_some(),
        }
}

/// What the move size reads: the fine step, a size in beats or the loop.
fn move_size_text(cx: &Cx<'_>, snapshot: &DeckSnapshot) -> String {
    let controls = &snapshot.controls;
    if controls.move_fine {
        return cx.texts.get("deck.move_fine_step").to_owned();
    }
    match controls.move_size {
        MoveSize::Beats(size) => waveform::size_text(cx.texts, size),
        MoveSize::Loop => cx.texts.get("deck.move_size_loop").to_owned(),
    }
}

/// The loop row, above the page of the advanced panel: the loop size between its arrows,
/// In, Out and Active, and at the right end the beatjump keys around the move size.
fn loop_controls(cx: &mut Cx<'_>, area: Rect, deck: DeckId, snapshot: &DeckSnapshot) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let letter = deck.letter().to_string();
    let named = |key: &str| texts.format(key, &[("deck", &letter)]);
    let controls = &snapshot.controls;
    let looping = &snapshot.looping;
    let loaded = is_ready(snapshot);
    let has_tempo = snapshot.grid.bpm().is_some();
    let on = looping.region.is_some();
    let lit = tokens.color.loop_region;
    let (first_top, _) = page_rows(area, tokens);
    let top = first_top - tokens.space.s3 - LOOP_ROW_HEIGHT;
    let gap = tokens.space.s2;
    let mut row = Row {
        x: area.left(),
        top,
        height: LOOP_ROW_HEIGHT,
        gap,
    };

    let size = controls.loop_size;
    let size_text = waveform::size_text(texts, size);
    let smaller = named("deck.loop_smaller_name");
    let larger = named("deck.loop_larger_name");
    let step = |direction| Command::DeckLoopSizeStep { deck, direction }.into();
    stepper(
        cx,
        &mut row,
        ("deck-loop-size", deck.index()),
        (&size_text, tokens.text.tempo, LOOP_SIZE_WIDTH),
        [
            Arrow {
                name: &smaller,
                enabled: size.smaller().is_some(),
                action: step(SizeStep::Down),
            },
            Arrow {
                name: &larger,
                enabled: size.larger().is_some(),
                action: step(SizeStep::Up),
            },
        ],
    );
    row.x += tokens.space.s3;
    let keys = [
        (
            "deck.loop_in",
            "deck.loop_in_name",
            Command::DeckLoopIn { deck },
            loaded,
            looping.in_point.is_some(),
        ),
        (
            "deck.loop_out",
            "deck.loop_out_name",
            Command::DeckLoopOut { deck },
            loaded && (on || looping.in_point.is_some()),
            false,
        ),
        (
            "deck.loop_active",
            "deck.loop_active_name",
            Command::DeckLoopToggle { deck },
            loaded && (on || has_tempo),
            on,
        ),
    ];
    for (index, (label, name, command, enabled, is_lit)) in keys.into_iter().enumerate() {
        let label = texts.get(label);
        let rect = row.place(key::width_for(cx, label, 36.0));
        let name = named(name);
        let mut spec = KeySpec::text(label).enabled(enabled);
        spec.name = &name;
        spec.lit = is_lit.then_some(lit);
        key::key(
            cx,
            rect,
            id(("deck-loop-key", deck.index(), index)),
            spec,
            Press::Click(command.into()),
        );
    }

    // The beatjump: back, the move size, forward, at the right end of the row.
    let jump = loaded && step_has_length(snapshot);
    let reading = move_size_text(cx, snapshot);
    let mut row = Row {
        x: area.right() - 2.0 * (ARROW_WIDTH + gap) - MOVE_SIZE_WIDTH,
        top,
        height: LOOP_ROW_HEIGHT,
        gap,
    };
    let jumps = [
        ("deck.jump_back", "deck.jump_back_name", Direction::Back),
        (
            "deck.jump_forward",
            "deck.jump_forward_name",
            Direction::Forward,
        ),
    ];
    for (index, (label, name, direction)) in jumps.into_iter().enumerate() {
        if index == 1 {
            let rect = row.place(MOVE_SIZE_WIDTH);
            let ink = if jump {
                tokens.color.text_primary
            } else {
                tokens.color.text_disabled
            };
            paint::text(
                cx.painter(),
                theme,
                rect,
                Align2::CENTER_CENTER,
                &reading,
                tokens.text.readout,
                ink,
            );
        }
        let rect = row.place(ARROW_WIDTH);
        let name = named(name);
        let mut spec = KeySpec::text(texts.get(label)).enabled(jump);
        spec.name = &name;
        key::key(
            cx,
            rect,
            id(("deck-jump", deck.index(), index)),
            spec,
            Press::Click(Command::DeckBeatjump { deck, direction }.into()),
        );
    }
}

/// The two rows of the MOVE page: what the moves act on; then the move size between its
/// arrows, the two move keys and the fine steps.
fn move_page(cx: &mut Cx<'_>, area: Rect, deck: DeckId, snapshot: &DeckSnapshot) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let letter = deck.letter().to_string();
    let named = |key: &str| texts.format(key, &[("deck", &letter)]);
    let controls = &snapshot.controls;
    let loaded = is_ready(snapshot);
    let (first_top, second_top) = page_rows(area, tokens);
    let gap = tokens.space.s2;

    let modes = [
        "deck.move_mode_beatjump",
        "deck.move_mode_loop",
        "deck.move_mode_loop_in",
        "deck.move_mode_loop_out",
    ]
    .map(|key| Choice {
        label: texts.get(key),
        enabled: true,
    });
    let first = Rect::from_min_size(
        egui::pos2(area.left(), first_top),
        egui::vec2(area.width(), GRID_ROW_HEIGHT),
    );
    choice::segmented(
        cx,
        first,
        id(("deck-move-mode", deck.index())),
        &modes,
        MoveMode::ALL
            .iter()
            .position(|mode| *mode == controls.move_mode),
        |index| {
            Command::DeckMoveModeSet {
                deck,
                mode: MoveMode::ALL[index],
            }
            .into()
        },
    );

    let mut row = Row {
        x: area.left(),
        top: second_top,
        height: GRID_ROW_HEIGHT,
        gap,
    };
    let reading = move_size_text(cx, snapshot);
    let smaller = named("deck.move_smaller_name");
    let larger = named("deck.move_larger_name");
    let step = |direction| Command::DeckMoveSizeStep { deck, direction }.into();
    let can_step =
        |direction| !controls.move_fine && controls.move_size.stepped(direction).is_some();
    stepper(
        cx,
        &mut row,
        ("deck-move-size", deck.index()),
        (&reading, tokens.text.readout, MOVE_SIZE_WIDTH),
        [
            Arrow {
                name: &smaller,
                enabled: can_step(SizeStep::Down),
                action: step(SizeStep::Down),
            },
            Arrow {
                name: &larger,
                enabled: can_step(SizeStep::Up),
                action: step(SizeStep::Up),
            },
        ],
    );
    row.x += tokens.space.s3;
    // A move of the loop needs the loop.
    let moves = loaded
        && step_has_length(snapshot)
        && (controls.move_mode == MoveMode::Beatjump || snapshot.looping.region.is_some());
    let keys = [
        ("deck.jump_back", "deck.move_back_name", Direction::Back),
        (
            "deck.jump_forward",
            "deck.move_forward_name",
            Direction::Forward,
        ),
    ];
    for (index, (label, name, direction)) in keys.into_iter().enumerate() {
        let rect = row.place(ARROW_WIDTH);
        let name = named(name);
        let mut spec = KeySpec::text(texts.get(label)).enabled(moves);
        spec.name = &name;
        key::key(
            cx,
            rect,
            id(("deck-move", deck.index(), index)),
            spec,
            Press::Click(Command::DeckMove { deck, direction }.into()),
        );
    }
    row.x += tokens.space.s3;
    let fine = texts.get("deck.move_fine");
    let rect = row.place(key::width_for(cx, fine, 36.0));
    let name = named("deck.move_fine_name");
    let mut spec = KeySpec::text(fine);
    spec.name = &name;
    spec.lit = controls.move_fine.then_some(tokens.color.text_primary);
    key::key(
        cx,
        rect,
        id(("deck-move-fine", deck.index())),
        spec,
        Press::Click(Command::DeckMoveFineToggle { deck }.into()),
    );
}

/// The translation key of the name of a kind of cue.
fn cue_kind_key(kind: CueKind) -> &'static str {
    match kind {
        CueKind::Cue => "deck.cue_type_cue",
        CueKind::FadeIn => "deck.cue_type_fade_in",
        CueKind::FadeOut => "deck.cue_type_fade_out",
        CueKind::Load => "deck.cue_type_load",
        CueKind::Loop => "deck.cue_type_loop",
    }
}

/// What a cue is shown by: its name, or its kind until it has one.
fn cue_label<'a>(texts: &'a Texts, cue: &'a Cue) -> &'a str {
    if cue.name.is_empty() {
        texts.get(cue_kind_key(cue.kind()))
    } else {
        cue.name.as_str()
    }
}

/// What the pointer or the keyboard holds a pad down for.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PadHold {
    #[default]
    Up,
    /// The cue is being played or stored: the release is sent too.
    Pressed,
    /// The press deleted the cue: the release sends nothing.
    Deleted,
}

/// The eight hotcues, two rows of four above the loop row. A pad acts while held, as the
/// cue key does; with Shift it deletes its cue; in the mapping mode it takes the selected
/// cue instead.
fn hotcue_pads(cx: &mut Cx<'_>, area: Rect, deck: DeckId, snapshot: &DeckSnapshot, mapping: bool) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let letter = deck.letter().to_string();
    let loaded = is_ready(snapshot);
    let gap = tokens.space.s2;
    let (first_top, _) = page_rows(area, tokens);
    let bottom = first_top - tokens.space.s3 - LOOP_ROW_HEIGHT - gap;
    let top = bottom - 2.0 * PAD_HEIGHT - gap;
    let columns = PAD_COLUMNS as f32;
    let width = (area.width() - (columns - 1.0) * gap) / columns;
    let mut bold = tokens.text.button;
    bold.weight = Weight::Bold;
    for slot in HotcueSlot::ALL {
        let index = slot.index();
        let rect = Rect::from_min_size(
            egui::pos2(
                area.left() + (index % PAD_COLUMNS) as f32 * (width + gap),
                top + (index / PAD_COLUMNS) as f32 * (PAD_HEIGHT + gap),
            ),
            egui::vec2(width, PAD_HEIGHT),
        );
        let cue = snapshot.cues.get(slot).filter(|_| loaded);
        let number = slot.number().to_string();
        let values = [("slot", number.as_str()), ("deck", letter.as_str())];
        let (name, command, kind, enabled) = if mapping {
            (
                "deck.hotcue_map_name",
                "deck.hotcue.map",
                ControlKind::Button,
                loaded && snapshot.cues.selected().is_some(),
            )
        } else if cue.is_some() {
            (
                "deck.hotcue_name",
                "deck.hotcue.trigger",
                ControlKind::Hold,
                loaded,
            )
        } else {
            (
                "deck.hotcue_empty_name",
                "deck.hotcue.trigger",
                ControlKind::Hold,
                loaded,
            )
        };
        let name = texts.format(name, &values);
        let pad = id(("deck-hotcue", deck.index(), index));
        cx.record(command, rect, kind, enabled);
        let response = cx.ui.interact(rect, pad, cx.sense(enabled, Sense::click()));
        response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, &name));
        let focused = response.has_focus();
        if focused {
            cx.out.claim(&[Key::Space, Key::Enter]);
        }
        let mut down = false;
        if mapping {
            if response.clicked() {
                cx.out.emit(Command::DeckHotcueMap { deck, slot });
                cx.out.emit(AppCommand::DeckCueMapToggle { deck });
            }
        } else {
            let (keyboard, shift) = cx.ui.input(|input| {
                let keys = input.key_down(Key::Space) || input.key_down(Key::Enter);
                (focused && keys, input.modifiers.shift)
            });
            down = enabled && (response.is_pointer_button_down_on() || keyboard);
            let held = cx
                .ui
                .ctx()
                .data(|data| data.get_temp::<PadHold>(pad))
                .unwrap_or_default();
            if down != (held != PadHold::Up) {
                let next = match (down, shift && cue.is_some()) {
                    (false, _) => PadHold::Up,
                    (true, true) => PadHold::Deleted,
                    (true, false) => PadHold::Pressed,
                };
                cx.ui.ctx().data_mut(|data| data.insert_temp(pad, next));
                let trigger = |pressed| Command::DeckHotcueTrigger {
                    deck,
                    slot,
                    pressed,
                };
                match (held, next) {
                    (_, PadHold::Deleted) => cx.out.emit(Command::DeckHotcueDelete { deck, slot }),
                    (_, PadHold::Pressed) => cx.out.emit(trigger(true)),
                    (PadHold::Pressed, PadHold::Up) => cx.out.emit(trigger(false)),
                    _ => {}
                }
            }
        }

        let painter = cx.painter();
        let corner = paint::radius(tokens.radius.lg);
        let hovered = enabled && response.hovered();
        let ink = tokens.hotcue[index];
        let numeral = Rect::from_min_max(
            egui::pos2(rect.left() + tokens.space.s3, rect.top() + PAD_STRIP),
            egui::pos2(rect.left() + tokens.space.s3 + 12.0, rect.bottom()),
        );
        match cue {
            Some(cue) => {
                let fill = if down {
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
                let strip = Rect::from_min_size(rect.min, egui::vec2(rect.width(), PAD_STRIP));
                painter.rect_filled(
                    strip,
                    CornerRadius {
                        nw: corner.nw,
                        ne: corner.ne,
                        sw: 0,
                        se: 0,
                    },
                    ink,
                );
                paint::text(
                    painter,
                    theme,
                    numeral,
                    Align2::LEFT_CENTER,
                    &number,
                    bold,
                    ink,
                );
                let label = Rect::from_min_max(
                    egui::pos2(numeral.right() + tokens.space.s1, numeral.top()),
                    egui::pos2(rect.right() - tokens.space.s2, rect.bottom()),
                );
                paint::text_elided(
                    painter,
                    theme,
                    label,
                    cue_label(texts, cue),
                    tokens.text.body,
                    color.text_primary,
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
                let ink = if loaded {
                    color.text_secondary
                } else {
                    color.text_disabled
                };
                paint::text(
                    painter,
                    theme,
                    numeral,
                    Align2::LEFT_CENTER,
                    &number,
                    tokens.text.button,
                    ink,
                );
            }
        }
        if mapping && enabled {
            let outline = [
                rect.left_top(),
                rect.right_top(),
                rect.right_bottom(),
                rect.left_bottom(),
                rect.left_top(),
            ];
            painter.extend(Shape::dashed_line(
                &outline,
                Stroke::new(tokens.stroke.focus, color.focus_ring),
                4.0,
                3.0,
            ));
        }
        if focused {
            paint::focus_ring(painter, theme, rect, tokens.radius.lg);
        }
    }
}

/// The two rows of the CUE page: the keys to the cue before and after, the selected cue
/// and Store, Map and Delete; then the type of the selected cue.
fn cue_page(cx: &mut Cx<'_>, area: Rect, deck: DeckId, snapshot: &DeckSnapshot, mapping: bool) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let letter = deck.letter().to_string();
    let named = |key: &str| texts.format(key, &[("deck", &letter)]);
    let loaded = is_ready(snapshot);
    let cues = &snapshot.cues;
    let selected = cues
        .selected()
        .filter(|_| loaded)
        .and_then(|slot| Some((slot, cues.get(slot)?)));
    let at = snapshot.position_seconds;
    let (first_top, second_top) = page_rows(area, tokens);
    let gap = tokens.space.s2;
    let height = GRID_ROW_HEIGHT;

    let mut row = Row {
        x: area.left(),
        top: first_top,
        height,
        gap,
    };
    let steps = [
        (
            Glyph::ChevronLeft,
            "deck.cue_previous_name",
            CueStep::Previous,
            cues.previous(at).is_some(),
        ),
        (
            Glyph::ChevronRight,
            "deck.cue_next_name",
            CueStep::Next,
            cues.next(at).is_some(),
        ),
    ];
    for (index, (glyph, name, direction, there)) in steps.into_iter().enumerate() {
        let rect = row.place(GRID_KEY_WIDTH);
        let name = named(name);
        key::key(
            cx,
            rect,
            id(("deck-cue-step", deck.index(), index)),
            KeySpec::icon(glyph, 14.0, &name).enabled(loaded && there),
            Press::Click(Command::DeckCueSelect { deck, direction }.into()),
        );
    }
    let reading_left = row.x + tokens.space.s2;

    // Store, Map and Delete at the right end of the row.
    let store = texts.get("deck.cue_store");
    let map = texts.get("deck.cue_map");
    let store_width = key::width_for(cx, store, GRID_KEY_WIDTH);
    let map_width = key::width_for(cx, map, GRID_KEY_WIDTH);
    let mut row = Row {
        x: area.right() - store_width - map_width - GRID_KEY_WIDTH - 2.0 * gap,
        top: first_top,
        height,
        gap,
    };
    let reading_right = row.x - tokens.space.s3;
    let rect = row.place(store_width);
    let name = named("deck.cue_store_name");
    let mut spec = KeySpec::text(store).enabled(loaded && cues.first_free().is_some());
    spec.name = &name;
    key::key(
        cx,
        rect,
        id(("deck-cue-store", deck.index())),
        spec,
        Press::Click(Command::DeckCueStore { deck }.into()),
    );
    let rect = row.place(map_width);
    let name = named("deck.cue_map_name");
    let mut spec = KeySpec::text(map).enabled(mapping || selected.is_some());
    spec.name = &name;
    spec.lit = mapping.then_some(color.focus_ring);
    key::key(
        cx,
        rect,
        id(("deck-cue-map", deck.index())),
        spec,
        Press::Click(AppCommand::DeckCueMapToggle { deck }.into()),
    );
    let rect = row.place(GRID_KEY_WIDTH);
    let name = named("deck.cue_delete_name");
    key::key(
        cx,
        rect,
        id(("deck-cue-delete", deck.index())),
        KeySpec::icon(Glyph::Trash, 14.0, &name).enabled(selected.is_some()),
        Press::Click(Command::DeckCueDelete { deck }.into()),
    );

    // Between them the selected cue: the colour and the number of its slot, what it is
    // called and where it is.
    let reading = Rect::from_min_max(
        egui::pos2(reading_left, first_top),
        egui::pos2(reading_right, first_top + height),
    );
    let painter = cx.painter();
    match selected {
        Some((slot, cue)) => {
            let square = Rect::from_center_size(
                egui::pos2(reading.left() + CUE_SQUARE / 2.0, reading.center().y),
                egui::vec2(CUE_SQUARE, CUE_SQUARE),
            );
            painter.rect_filled(
                square,
                paint::radius(tokens.radius.sm),
                tokens.hotcue[slot.index()],
            );
            let numeral = Rect::from_min_max(
                egui::pos2(square.right() + tokens.space.s2, reading.top()),
                egui::pos2(square.right() + tokens.space.s2 + 10.0, reading.bottom()),
            );
            paint::text(
                painter,
                theme,
                numeral,
                Align2::LEFT_CENTER,
                &slot.number().to_string(),
                tokens.text.body,
                color.text_primary,
            );
            let position = Rect::from_min_max(
                egui::pos2(reading.right() - CUE_POSITION_WIDTH, reading.top()),
                reading.max,
            );
            paint::text(
                painter,
                theme,
                position,
                Align2::RIGHT_CENTER,
                &format::position(cue.position),
                tokens.text.readout,
                color.text_secondary,
            );
            let label = Rect::from_min_max(
                egui::pos2(numeral.right() + tokens.space.s1, reading.top()),
                egui::pos2(position.left() - tokens.space.s2, reading.bottom()),
            );
            paint::text_elided(
                painter,
                theme,
                label,
                cue_label(texts, cue),
                tokens.text.body,
                color.text_primary,
            );
        }
        None => {
            paint::text_elided(
                painter,
                theme,
                reading,
                texts.get("deck.cue_none"),
                tokens.text.body,
                color.text_disabled,
            );
        }
    }

    // Second row: the type. A cue becomes a loop of the selected size, which needs a tempo.
    let has_tempo = snapshot.grid.bpm().is_some();
    let kind = selected.map(|(_, cue)| cue.kind());
    let choices = CueKind::ALL.map(|option| Choice {
        label: texts.get(cue_kind_key(option)),
        enabled: option != CueKind::Loop || has_tempo || kind == Some(CueKind::Loop),
    });
    let name = named("deck.cue_type_name");
    choice::select(
        cx,
        Rect::from_min_size(
            egui::pos2(area.left(), second_top),
            egui::vec2(CUE_TYPE_WIDTH, height),
        ),
        id(("deck-cue-type", deck.index())),
        SelectSpec {
            name: &name,
            choices: &choices,
            current: kind.and_then(|kind| CueKind::ALL.iter().position(|other| *other == kind)),
            enabled: selected.is_some(),
        },
        |index| {
            Command::DeckCueTypeSet {
                deck,
                kind: CueKind::ALL[index],
            }
            .into()
        },
    );
}

/// The switch between the pages of the advanced panel, at the right end of their second
/// row.
fn page_selector(cx: &mut Cx<'_>, area: Rect, deck: DeckId, page: DeckPage) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let (_, second_top) = page_rows(area, tokens);
    let pages = ["deck.page_move", "deck.page_cue", "deck.page_grid"].map(|key| Choice {
        label: texts.get(key),
        enabled: true,
    });
    // The width the segmented control gives itself: its labels, their padding and its
    // outline.
    let width: f32 = pages
        .iter()
        .map(|page| {
            let text = paint::text_width(cx.painter(), theme, page.label, tokens.text.button);
            (text + 2.0 * tokens.space.s4).ceil()
        })
        .sum::<f32>()
        + 2.0;
    let rect = Rect::from_min_size(
        egui::pos2(area.right() - width, second_top),
        egui::vec2(width, GRID_ROW_HEIGHT),
    );
    choice::segmented(
        cx,
        rect,
        id(("deck-page", deck.index())),
        &pages,
        DeckPage::ALL.iter().position(|other| *other == page),
        |index| {
            AppCommand::DeckPageSet {
                deck,
                page: DeckPage::ALL[index],
            }
            .into()
        },
    );
}

/// Places keys one after another along a row.
struct Row {
    x: f32,
    top: f32,
    height: f32,
    gap: f32,
}

impl Row {
    fn place(&mut self, width: f32) -> Rect {
        let rect =
            Rect::from_min_size(egui::pos2(self.x, self.top), egui::vec2(width, self.height));
        self.x += width + self.gap;
        rect
    }
}

/// The two rows of the GRID page: the tempo with its step, scale and tap keys, the marker
/// nudge and the lock; then the marker keys.
fn grid_page(cx: &mut Cx<'_>, area: Rect, deck: DeckId, snapshot: &DeckSnapshot) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let letter = deck.letter().to_string();
    let grid = &snapshot.grid;
    let loaded = is_ready(snapshot);
    let editable = loaded && !grid.is_locked();
    let has_tempo = grid.bpm().is_some();
    let has_markers = !grid.markers().is_empty();
    let height = GRID_ROW_HEIGHT;
    let (first_top, second_top) = page_rows(area, tokens);
    let gap = tokens.space.s2;
    let named = |key: &str| texts.format(key, &[("deck", &letter)]);

    // The tempo field starts from the tempo of the grid, or of the tag, or 120 BPM.
    let shown = grid.bpm().map(|bpm| bpm.value());
    let start = shown
        .or_else(|| snapshot.track.as_ref().and_then(|track| track.tags.bpm))
        .filter(|bpm| (Bpm::MIN..=Bpm::MAX).contains(bpm))
        .unwrap_or(DEFAULT_FIELD_BPM);
    let span = Bpm::MAX - Bpm::MIN;
    let field_rect = Rect::from_min_size(
        egui::pos2(area.left(), first_top),
        egui::vec2(BPM_FIELD_WIDTH, height),
    );
    let field_text = shown.map_or_else(|| texts.get("deck.no_tempo").to_owned(), format::bpm);
    let name = named("deck.grid_bpm_name");
    value::field(
        cx,
        field_rect,
        id(("deck-grid-bpm", deck.index())),
        ValueSpec {
            name: &name,
            position: (start - Bpm::MIN) / span,
            default: (start - Bpm::MIN) / span,
            steps: Steps {
                small: BPM_STEP / span,
                fine: BPM_STEP / 10.0 / span,
                large: 1.0 / span,
            },
            enabled: editable,
        },
        &field_text,
        |position| {
            Command::DeckGridBpmSet {
                deck,
                bpm: Bpm::MIN + position * span,
            }
            .into()
        },
    );

    let mut row = Row {
        x: field_rect.right() + gap,
        top: first_top,
        height,
        gap,
    };
    let steps = [
        (Glyph::Minus, "deck.grid_slower_name", -BPM_STEP),
        (Glyph::Plus, "deck.grid_faster_name", BPM_STEP),
    ];
    for (index, (glyph, name, amount)) in steps.into_iter().enumerate() {
        let rect = row.place(GRID_KEY_WIDTH);
        let name = named(name);
        key::key(
            cx,
            rect,
            id(("deck-grid-step", deck.index(), index)),
            KeySpec::icon(glyph, 14.0, &name).enabled(editable && has_tempo),
            Press::Click(Command::DeckGridBpmStep { deck, amount }.into()),
        );
    }
    let text_keys = [
        (
            "deck.grid_half",
            "deck.grid_half_name",
            Command::DeckGridBpmScale {
                deck,
                factor: BpmScale::Half,
            },
            editable && has_tempo,
        ),
        (
            "deck.grid_double",
            "deck.grid_double_name",
            Command::DeckGridBpmScale {
                deck,
                factor: BpmScale::Double,
            },
            editable && has_tempo,
        ),
        (
            "deck.grid_tap",
            "deck.grid_tap_name",
            Command::DeckGridTap { deck },
            editable,
        ),
    ];
    for (index, (label, name, command, enabled)) in text_keys.into_iter().enumerate() {
        let label = texts.get(label);
        let rect = row.place(key::width_for(cx, label, GRID_KEY_WIDTH));
        let name = named(name);
        let mut spec = KeySpec::text(label).enabled(enabled);
        spec.name = &name;
        key::key(
            cx,
            rect,
            id(("deck-grid-key", deck.index(), index)),
            spec,
            Press::Click(command.into()),
        );
    }
    let nudges = [
        (
            Glyph::ChevronLeft,
            "deck.grid_earlier_name",
            -MARKER_STEP_SECONDS,
        ),
        (
            Glyph::ChevronRight,
            "deck.grid_later_name",
            MARKER_STEP_SECONDS,
        ),
    ];
    for (index, (glyph, name, seconds)) in nudges.into_iter().enumerate() {
        let rect = row.place(GRID_KEY_WIDTH);
        let name = named(name);
        key::key(
            cx,
            rect,
            id(("deck-grid-nudge", deck.index(), index)),
            KeySpec::icon(glyph, 14.0, &name).enabled(editable && has_markers),
            Press::Click(Command::DeckGridMarkerMove { deck, seconds }.into()),
        );
    }
    let lock = Rect::from_min_size(
        egui::pos2(area.right() - GRID_KEY_WIDTH, first_top),
        egui::vec2(GRID_KEY_WIDTH, height),
    );
    let name = named("deck.grid_lock_name");
    let mut spec = KeySpec::icon(Glyph::Lock, 14.0, &name).enabled(loaded);
    spec.lit = grid.is_locked().then_some(deck_color(theme, deck));
    key::key(
        cx,
        lock,
        id(("deck-grid-lock", deck.index())),
        spec,
        Press::Click(Command::DeckGridLockToggle { deck }.into()),
    );

    // Second row: the markers.
    let mut row = Row {
        x: area.left(),
        top: second_top,
        height,
        gap,
    };
    let markers = [
        (
            "deck.grid_previous",
            "deck.grid_previous_name",
            Command::DeckGridMarkerPrevious { deck },
            loaded && has_markers,
        ),
        (
            "deck.grid_set",
            "deck.grid_set_name",
            Command::DeckGridMarkerSet { deck },
            editable,
        ),
        (
            "deck.grid_next",
            "deck.grid_next_name",
            Command::DeckGridMarkerNext { deck },
            loaded && has_markers,
        ),
    ];
    for (index, (label, name, command, enabled)) in markers.into_iter().enumerate() {
        let label = texts.get(label);
        let rect = row.place(key::width_for(cx, label, GRID_KEY_WIDTH));
        let name = named(name);
        let mut spec = KeySpec::text(label).enabled(enabled);
        spec.name = &name;
        key::key(
            cx,
            rect,
            id(("deck-grid-marker", deck.index(), index)),
            spec,
            Press::Click(command.into()),
        );
    }
    let rect = row.place(GRID_KEY_WIDTH);
    let name = named("deck.grid_delete_name");
    key::key(
        cx,
        rect,
        id(("deck-grid-delete", deck.index())),
        KeySpec::icon(Glyph::Trash, 14.0, &name).enabled(editable && has_markers),
        Press::Click(Command::DeckGridMarkerDelete { deck }.into()),
    );
}

fn header(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let snapshot = &view.engine.decks[deck.index()];
    let letter = deck.letter().to_string();
    let badge = Rect::from_min_size(rect.min, egui::vec2(22.0, 22.0));
    paint::badge(
        painter,
        theme,
        badge,
        deck.letter(),
        deck_color(theme, deck),
    );

    // Right to left: the times, then the tempo; the texts take what is left.
    let time_block = Rect::from_min_max(egui::pos2(rect.right() - 96.0, rect.top()), rect.max);
    let tempo_block = Rect::from_min_max(
        egui::pos2(time_block.left() - tokens.space.s4 - 96.0, rect.top()),
        egui::pos2(time_block.left() - tokens.space.s4, rect.bottom()),
    );
    let text_block = Rect::from_min_max(
        egui::pos2(badge.right() + tokens.space.s4, rect.top()),
        egui::pos2(tempo_block.left() - tokens.space.s4, rect.bottom()),
    );
    let line = |block: Rect, top: f32, height: f32| {
        Rect::from_min_size(
            egui::pos2(block.left(), block.top() + top),
            egui::vec2(block.width(), height),
        )
    };
    let body = tokens.text.body;
    let track = snapshot.track.as_ref();
    let loaded = is_ready(snapshot);

    // Title, artist and album; or what the deck is waiting for.
    let title_line = line(text_block, 0.0, 18.0);
    match (title_of(snapshot), snapshot.loading) {
        (Some(title), _) => {
            let elided = paint::text_elided(
                painter,
                theme,
                title_line,
                &title,
                tokens.text.title,
                color.text_primary,
            );
            let response =
                cx.ui
                    .interact(title_line, id(("deck-title", deck.index())), Sense::hover());
            response.widget_info(|| WidgetInfo::labeled(Role::Label, true, &title));
            if elided {
                response.on_hover_text(title);
            }
            let tags = track.map(|track| &track.tags);
            let artist = tags
                .and_then(|tags| tags.artist.as_deref())
                .unwrap_or_default();
            paint::text_elided(
                painter,
                theme,
                line(text_block, 18.0, 18.0),
                artist,
                body,
                color.text_secondary,
            );
            let third = if snapshot.loading {
                texts.get("deck.loading")
            } else {
                tags.and_then(|tags| tags.album.as_deref())
                    .unwrap_or_default()
            };
            paint::text_elided(
                painter,
                theme,
                line(text_block, 36.0, 18.0),
                third,
                body,
                color.text_secondary,
            );
        }
        (None, true) => {
            let loading = texts.get("deck.loading");
            paint::text_elided(
                painter,
                theme,
                title_line,
                loading,
                tokens.text.title,
                color.text_secondary,
            );
        }
        (None, false) => {
            let title = texts.get("deck.empty_title");
            paint::text_elided(
                painter,
                theme,
                title_line,
                title,
                tokens.text.title,
                color.text_secondary,
            );
            let chord = view
                .keymap
                .bindings()
                .iter()
                .find(|binding| {
                    binding.command == "browser.load_selected"
                        && binding.args.get("deck")
                            == Some(&platter_engine::command::Value::Text(
                                letter.to_ascii_lowercase(),
                            ))
                })
                .map(|binding| crate::panels::settings::chord_text(texts, &binding.chord));
            let hint = match &chord {
                Some(chord) => texts.format("deck.empty_hint", &[("shortcut", chord)]),
                None => texts.get("deck.empty_hint_no_shortcut").to_owned(),
            };
            let area = Rect::from_min_max(title_line.left_bottom(), text_block.max);
            paint::text_wrapped(painter, theme, area, &hint, body, color.text_secondary);
        }
    }

    // Tempo: the current tempo, and the fader offset with the base tempo.
    let controls = &snapshot.controls;
    let offset = format::signed_percent(controls.tempo_ratio - 1.0);
    // The tempo of the grid; until there is one, the tempo the tag gives.
    let base = snapshot
        .grid
        .bpm()
        .map(|bpm| bpm.value())
        .or_else(|| track.and_then(|track| track.tags.bpm))
        .filter(|_| loaded);
    let ink = |loaded_ink| {
        if loaded {
            loaded_ink
        } else {
            color.text_disabled
        }
    };
    let (tempo, detail) = match base {
        Some(base) => (
            format::bpm(base * controls.tempo_ratio),
            texts.format(
                "deck.tempo_of",
                &[("offset", &offset), ("base", &format::bpm(base))],
            ),
        ),
        None => (
            texts.get("deck.no_tempo").to_owned(),
            texts.format("deck.tempo_offset", &[("offset", &offset)]),
        ),
    };
    let tempo_ink = if base.is_some() {
        color.text_primary
    } else {
        color.text_disabled
    };
    paint::text(
        painter,
        theme,
        line(tempo_block, 0.0, 22.0),
        Align2::RIGHT_CENTER,
        &tempo,
        tokens.text.tempo,
        tempo_ink,
    );
    paint::text(
        painter,
        theme,
        line(tempo_block, 22.0, 14.0),
        Align2::RIGHT_CENTER,
        &detail,
        tokens.text.readout,
        ink(color.text_secondary),
    );

    // Times: one large, the other two below it. A click swaps remaining and elapsed.
    let action = AppCommand::DeckTimeDisplayToggle { deck };
    cx.record(action.id(), time_block, ControlKind::Button, loaded);
    let response = cx.ui.interact(
        time_block,
        id(("deck-time", deck.index())),
        cx.sense(loaded, Sense::click()),
    );
    let name = texts.format("deck.time_name", &[("deck", &letter)]);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, loaded, &name));
    if response.has_focus() {
        cx.out.claim(&[egui::Key::Space, egui::Key::Enter]);
        paint::focus_ring(painter, theme, time_block, tokens.radius.md);
    }
    if response.clicked() {
        cx.out.emit(action);
    }
    let none = texts.get("deck.no_time");
    let (large, second, third, large_ink) = match track {
        Some(track) => {
            let elapsed = snapshot.position_seconds.min(track.seconds);
            let left = (track.seconds - elapsed).max(0.0);
            let remaining = format::time(left, true);
            let elapsed = format::time(elapsed, false);
            let total = format::time(track.seconds, false);
            let ending = snapshot.playing && left < ENDING_SECONDS;
            if state.remaining_large[deck.index()] {
                let ink = if ending {
                    color.status_warn
                } else {
                    color.text_primary
                };
                (remaining, elapsed, total, ink)
            } else {
                (elapsed, remaining, total, color.text_primary)
            }
        }
        None => (
            none.to_owned(),
            none.to_owned(),
            String::new(),
            color.text_disabled,
        ),
    };
    let large_ink = if snapshot.loading {
        color.text_secondary
    } else {
        large_ink
    };
    paint::text(
        painter,
        theme,
        line(time_block, 0.0, 26.0),
        Align2::RIGHT_CENTER,
        &large,
        tokens.text.time,
        large_ink,
    );
    for (index, text) in [second, third].iter().enumerate() {
        paint::text(
            painter,
            theme,
            line(time_block, 26.0 + 14.0 * index as f32, 14.0),
            Align2::RIGHT_CENTER,
            text,
            tokens.text.readout,
            ink(color.text_secondary),
        );
    }
}

/// CUE, CUP and play or pause, along the bottom of `area`.
fn transport(cx: &mut Cx<'_>, area: Rect, deck: DeckId, snapshot: &DeckSnapshot) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let loaded = is_ready(snapshot);
    let lit = deck_color(theme, deck);
    let letter = deck.letter().to_string();
    let height = tokens.size.transport_h;
    let top = area.bottom() - height;
    let mut left = area.left();
    let mut place = |cx: &Cx<'_>, label: Option<&str>| {
        let width = match label {
            Some(label) => key::width_for(cx, label, tokens.size.button_transport),
            None => tokens.size.button_transport,
        };
        let rect = Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, height));
        left += width + tokens.space.s3;
        rect
    };

    let cue = texts.get("deck.cue");
    let rect = place(cx, Some(cue));
    let name = texts.format("deck.cue_name", &[("deck", &letter)]);
    let mut spec = KeySpec::text(cue).enabled(loaded);
    spec.name = &name;
    spec.held = Some(lit);
    key::key(
        cx,
        rect,
        id(("deck-cue", deck.index())),
        spec,
        Press::Hold {
            down: Command::DeckCue {
                deck,
                pressed: true,
            }
            .into(),
            up: Command::DeckCue {
                deck,
                pressed: false,
            }
            .into(),
        },
    );

    let cup = texts.get("deck.cup");
    let rect = place(cx, Some(cup));
    let name = texts.format("deck.cup_name", &[("deck", &letter)]);
    let mut spec = KeySpec::text(cup).enabled(loaded);
    spec.name = &name;
    spec.held = Some(lit);
    key::key(
        cx,
        rect,
        id(("deck-cup", deck.index())),
        spec,
        Press::Hold {
            down: Command::DeckCup {
                deck,
                pressed: true,
            }
            .into(),
            up: Command::DeckCup {
                deck,
                pressed: false,
            }
            .into(),
        },
    );

    let rect = place(cx, None);
    let (glyph, name) = if snapshot.playing {
        (Glyph::Pause, "deck.pause_name")
    } else {
        (Glyph::Play, "deck.play_name")
    };
    let name = texts.format(name, &[("deck", &letter)]);
    let spec = KeySpec {
        face: Face::Icon(glyph, 18.0),
        name: &name,
        lit: snapshot.playing.then_some(lit),
        held: None,
        enabled: loaded,
        text: None,
    };
    key::key(
        cx,
        rect,
        id(("deck-play", deck.index())),
        spec,
        Press::Click(Command::DeckPlayPause { deck }.into()),
    );
}

/// The tempo range, the tempo fader, its reading and the two bend keys, top to bottom.
fn tempo(cx: &mut Cx<'_>, column: Rect, deck: DeckId, snapshot: &DeckSnapshot) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let loaded = is_ready(snapshot);
    let controls = &snapshot.controls;
    let letter = deck.letter().to_string();
    let range = controls.tempo_range;
    let percent = format!("{:.0}", range * 100.0);

    let label = texts.format("deck.tempo_range", &[("percent", &percent)]);
    let name = texts.format("deck.tempo_range_name", &[("percent", &percent)]);
    let width = key::width_for(cx, &label, 0.0).max(43.0);
    let range_key = Rect::from_min_size(
        egui::pos2(column.center().x - width / 2.0, column.top()),
        egui::vec2(width, 24.0),
    );
    let mut spec = KeySpec::text(&label).enabled(loaded);
    spec.name = &name;
    key::key(
        cx,
        range_key,
        id(("deck-range", deck.index())),
        spec,
        Press::Click(Command::DeckTempoRangeCycle { deck }.into()),
    );

    let nudge_top = column.bottom() - 24.0 - 2.0;
    let reading_top = nudge_top - tokens.space.s3 - 14.0;
    let track = Rect::from_min_max(
        egui::pos2(
            column.center().x - FADER_WIDTH / 2.0,
            range_key.bottom() + tokens.space.s3,
        ),
        egui::pos2(
            column.center().x + FADER_WIDTH / 2.0,
            reading_top - tokens.space.s3,
        ),
    );
    let name = texts.format("deck.tempo_name", &[("deck", &letter)]);
    // Up is faster: the top of the travel is the top of the range.
    let position = 0.5 + (controls.tempo_ratio - 1.0) / (2.0 * range);
    let per_step = |percent: f64| percent / 100.0 / (2.0 * range);
    value::fader(
        cx,
        track,
        id(("deck-tempo", deck.index())),
        FaderSpec {
            value: ValueSpec {
                name: &name,
                position,
                default: 0.5,
                steps: Steps {
                    small: per_step(0.05),
                    fine: per_step(0.01),
                    large: per_step(1.0),
                },
                enabled: loaded,
            },
            mark: 0.5,
        },
        |position| {
            Command::DeckTempoSet {
                deck,
                ratio: 1.0 + (position - 0.5) * 2.0 * range,
            }
            .into()
        },
    );
    let offset = format::signed_percent(controls.tempo_ratio - 1.0);
    let reading = texts.format("deck.tempo_offset", &[("offset", &offset)]);
    let ink = if loaded {
        color.text_primary
    } else {
        color.text_disabled
    };
    let reading_rect = Rect::from_min_size(
        egui::pos2(column.left(), reading_top),
        egui::vec2(column.width(), 14.0),
    );
    paint::text(
        cx.painter(),
        theme,
        reading_rect,
        Align2::CENTER_CENTER,
        &reading,
        tokens.text.readout,
        ink,
    );

    let bends = [
        (Glyph::Minus, "deck.nudge_slower", -NUDGE_AMOUNT),
        (Glyph::Plus, "deck.nudge_faster", NUDGE_AMOUNT),
    ];
    for (index, (glyph, name, amount)) in bends.into_iter().enumerate() {
        let rect = Rect::from_min_size(
            egui::pos2(column.center().x - 30.0 + 32.0 * index as f32, nudge_top),
            egui::vec2(28.0, 24.0),
        );
        let mut spec = KeySpec::icon(glyph, 14.0, texts.get(name)).enabled(loaded);
        spec.held = Some(deck_color(theme, deck));
        key::key(
            cx,
            rect,
            id(("deck-nudge", deck.index(), index)),
            spec,
            Press::Hold {
                down: Command::DeckNudge { deck, amount }.into(),
                up: Command::DeckNudge { deck, amount: 0.0 }.into(),
            },
        );
    }
}

/// A message inside the deck about the last file sent to it.
fn deck_notice(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, notice: DeckNotice) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let (title, text) = match notice {
        DeckNotice::LoadFailed => ("deck.load_failed_title", "deck.load_failed_text"),
    };
    painter.rect_filled(rect, paint::radius(tokens.radius.md), color.surface_control);
    let inner = rect.shrink(tokens.space.s3);
    let icon = Rect::from_min_size(inner.min, egui::vec2(16.0, 16.0));
    theme
        .icons
        .paint(painter, Glyph::Error, icon, color.status_error);
    let dismiss = texts.get("deck.dismiss");
    let width = key::width_for(cx, dismiss, 0.0);
    let button = Rect::from_min_size(
        egui::pos2(
            inner.right() - width,
            inner.center().y - tokens.size.button_h / 2.0,
        ),
        egui::vec2(width, tokens.size.button_h),
    );
    let text_area = Rect::from_min_max(
        egui::pos2(icon.right() + tokens.space.s3, inner.top()),
        egui::pos2(button.left() - tokens.space.s3, inner.bottom()),
    );
    let mut bold = tokens.text.body;
    bold.weight = crate::tokens::Weight::SemiBold;
    let first = Rect::from_min_size(text_area.min, egui::vec2(text_area.width(), 18.0));
    paint::text_elided(
        painter,
        theme,
        first,
        texts.get(title),
        bold,
        color.text_primary,
    );
    let second = Rect::from_min_max(first.left_bottom(), text_area.max);
    paint::text_elided(
        painter,
        theme,
        second,
        texts.get(text),
        tokens.text.body,
        color.text_secondary,
    );
    let response = cx
        .ui
        .interact(text_area, id(("deck-notice", deck.index())), Sense::hover());
    let spoken = format!("{} {}", texts.get(title), texts.get(text));
    response.widget_info(|| WidgetInfo::labeled(Role::Alert, true, &spoken));
    key::key(
        cx,
        button,
        id(("deck-notice-dismiss", deck.index())),
        KeySpec::text(dismiss),
        Press::Click(AppCommand::DeckNoticeDismiss { deck }.into()),
    );
}
