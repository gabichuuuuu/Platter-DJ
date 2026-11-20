//! The loop recorder in the middle panel of the effect row, left of the mix recorder.
//!
//! Component: *Loop recorder (M4)* in `docs/design/DESIGN.md`. Behaviour note:
//! `docs/behavior/recording.md`, *Loop recorder*.

use egui::{Align2, Id, Key, Rect};
use platter_core::DeckId;
use platter_engine::{Command, LOOP_BARS, LoopPhase, LoopRecorderSnapshot, LoopSource, MixerMode};

use crate::command::Action;
use crate::cx::{Cx, id};
use crate::icons::Glyph;
use crate::model::View;
use crate::paint;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::key::{self, KeySpec, Press};
use crate::widgets::menu::{self, Item};
use crate::widgets::value::{self, KnobSpec, Steps, ValueSpec};

/// From the left edge of the middle panel.
pub const PADDING: f32 = 12.0;
/// Between the loop recorder and the mix recorder.
pub const GAP_TO_MIX: f32 = 16.0;
const LABEL_HEIGHT: f32 = 14.0;
const LABEL_GAP: f32 = 4.0;
const ROW: f32 = 28.0;
const SIZE_WIDTH: f32 = 52.0;
const REC_WIDTH: f32 = 40.0;
const PLAY_WIDTH: f32 = 36.0;
const UNDO_WIDTH: f32 = 32.0;
const KNOB: f32 = 28.0;
const BAR_GAP: f32 = 4.0;
const BAR_HEIGHT: f32 = 2.0;
const PLAY_ICON: f32 = 14.0;
/// The source selector: wide enough for `Deck A` in both languages.
const SOURCE_WIDTH: f32 = 72.0;

const SOURCES: [LoopSource; 5] = [
    LoopSource::Master,
    LoopSource::Deck(DeckId::A),
    LoopSource::Deck(DeckId::B),
    LoopSource::Deck(DeckId::C),
    LoopSource::Deck(DeckId::D),
];

/// The loop recorder in `rect`: its label and source over the size, Rec, play, Undo and the
/// dry / wet knob, and the progress bar under them.
pub fn panel(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>) {
    let looper = &view.engine.loop_recorder;
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let texts = cx.texts;
    // In external mixer mode the mix the loop plays into is not heard.
    let usable = view.engine.mixer.mode == MixerMode::Internal;
    let recorded = looper.seconds > 0.0;
    let height = LABEL_HEIGHT + LABEL_GAP + ROW + BAR_GAP + BAR_HEIGHT;
    let top = rect.center().y - height / 2.0;
    let left = rect.left();

    let label = paint::text(
        cx.painter(),
        theme,
        Rect::from_min_size(
            egui::pos2(left, top),
            egui::vec2(rect.width(), LABEL_HEIGHT),
        ),
        Align2::LEFT_CENTER,
        texts.get("loop_recorder.title"),
        tokens.text.label,
        color.text_secondary,
    );
    let source_names: Vec<String> = SOURCES
        .iter()
        .map(|source| source_name(cx, *source))
        .collect();
    let choices: Vec<Choice<'_>> = source_names
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let source_left = label.right() + tokens.space.s2;
    choice::label_select(
        cx,
        Rect::from_min_size(
            egui::pos2(source_left, top - 1.0),
            egui::vec2(
                SOURCE_WIDTH.min(rect.right() - source_left),
                LABEL_HEIGHT + 2.0,
            ),
        ),
        id("loop-source"),
        SelectSpec {
            name: texts.get("loop_recorder.source_name"),
            choices: &choices,
            current: SOURCES.iter().position(|source| *source == looper.source),
            enabled: usable,
        },
        |index| {
            Command::LoopRecorderSourceSet {
                source: SOURCES.get(index).copied().unwrap_or_default(),
            }
            .into()
        },
    );

    let row = top + LABEL_HEIGHT + LABEL_GAP;
    let widths = [SIZE_WIDTH, REC_WIDTH, PLAY_WIDTH, UNDO_WIDTH, KNOB];
    let gap = ((rect.width() - widths.iter().sum::<f32>()) / 4.0).max(tokens.space.s1);
    let mut x = left;
    let mut cells = widths.map(|width| {
        let cell = Rect::from_min_size(egui::pos2(x, row), egui::vec2(width, ROW));
        x += width + gap;
        cell
    });
    // The knob is square, centred in its cell.
    cells[4] = Rect::from_center_size(cells[4].center(), egui::vec2(KNOB, KNOB));
    let [size, rec, play, undo, knob] = cells;

    size_select(cx, size, looper, usable);
    rec_key(cx, rec, looper, usable);
    play_key(cx, play, looper, usable && recorded);
    undo_key(cx, undo, looper, usable && recorded);
    value::knob(
        cx,
        knob,
        id("loop-dry-wet"),
        KnobSpec {
            value: ValueSpec {
                name: texts.get("loop_recorder.dry_wet"),
                position: looper.dry_wet,
                default: 0.5,
                steps: Steps::PERCENT,
                enabled: usable && recorded,
            },
            size: KNOB,
            centre_detent: true,
            hide_arc: false,
        },
        |amount| Command::LoopRecorderDryWetSet { amount }.into(),
    );

    let bar = Rect::from_min_size(
        egui::pos2(left, row + ROW + BAR_GAP),
        egui::vec2(rect.width(), BAR_HEIGHT),
    );
    let painter = cx.painter();
    painter.rect_filled(bar, 0.0, color.line_key);
    let played = if looper.phase == LoopPhase::Recording {
        color.status_error
    } else {
        color.text_primary
    };
    let progress = looper.progress.clamp(0.0, 1.0);
    if recorded && progress > 0.0 {
        let mut filled = bar;
        filled.set_width(bar.width() * progress);
        painter.rect_filled(filled, 0.0, played);
    }
}

/// The name of a source: `Master` or the deck.
fn source_name(cx: &Cx<'_>, source: LoopSource) -> String {
    match source {
        LoopSource::Master => cx.texts.get("loop_recorder.master").to_owned(),
        LoopSource::Deck(deck) => cx.texts.format(
            "loop_recorder.deck",
            &[("deck", &deck.letter().to_string())],
        ),
    }
}

fn size_select(cx: &mut Cx<'_>, rect: Rect, looper: &LoopRecorderSnapshot, usable: bool) {
    let texts = cx.texts;
    let labels: Vec<String> = LOOP_BARS
        .iter()
        .map(|bars| {
            if *bars == 1 {
                texts.get("loop_recorder.one_bar").to_owned()
            } else {
                texts.format("loop_recorder.bars", &[("bars", &bars.to_string())])
            }
        })
        .collect();
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    choice::small_select(
        cx,
        rect,
        id("loop-size"),
        SelectSpec {
            name: texts.get("loop_recorder.size_name"),
            choices: &choices,
            current: LOOP_BARS.iter().position(|bars| *bars == looper.bars),
            // The size is chosen before a loop is recorded.
            enabled: usable && looper.phase == LoopPhase::Empty,
        },
        |index| {
            Command::LoopRecorderSizeSet {
                bars: f64::from(LOOP_BARS.get(index).copied().unwrap_or(4)),
            }
            .into()
        },
    );
}

fn rec_key(cx: &mut Cx<'_>, rect: Rect, looper: &LoopRecorderSnapshot, usable: bool) {
    let texts = cx.texts;
    let color = &cx.theme.tokens.color;
    let recording = matches!(
        looper.phase,
        LoopPhase::Waiting | LoopPhase::Recording | LoopPhase::Overdubbing
    );
    let mut spec = KeySpec::text(texts.get("loop_recorder.rec")).enabled(usable);
    spec.name = texts.get(if recording {
        "loop_recorder.stop_name"
    } else {
        "loop_recorder.rec_name"
    });
    spec.lit = recording.then_some(color.status_error);
    key::key(
        cx,
        rect,
        id("loop-rec"),
        spec,
        Press::Click(Command::LoopRecorderRecord.into()),
    );
}

fn play_key(cx: &mut Cx<'_>, rect: Rect, looper: &LoopRecorderSnapshot, enabled: bool) {
    let texts = cx.texts;
    let color = &cx.theme.tokens.color;
    let playing = matches!(looper.phase, LoopPhase::Playing | LoopPhase::Overdubbing);
    let (glyph, name) = if playing {
        (Glyph::Pause, texts.get("loop_recorder.pause_name"))
    } else {
        (Glyph::Play, texts.get("loop_recorder.play_name"))
    };
    let mut spec = KeySpec::icon(glyph, PLAY_ICON, name).enabled(enabled);
    spec.lit = playing.then_some(color.text_primary);
    key::key(
        cx,
        rect,
        id("loop-play"),
        spec,
        Press::Click(Command::LoopRecorderPlay.into()),
    );
}

/// Undo, whose context menu holds Redo and Delete.
fn undo_key(cx: &mut Cx<'_>, rect: Rect, looper: &LoopRecorderSnapshot, enabled: bool) {
    let texts = cx.texts;
    let key_id = id("loop-undo");
    let mut spec = KeySpec::text(texts.get("loop_recorder.undo")).enabled(enabled);
    spec.name = texts.get("loop_recorder.undo_name");
    let response = key::key_response(
        cx,
        rect,
        key_id,
        spec,
        Press::Click(Command::LoopRecorderUndo.into()),
    );
    let menu_id = id("loop-undo-menu");
    let keyed = response.has_focus()
        && cx
            .ui
            .input(|input| input.modifiers.shift && input.key_pressed(Key::F10));
    if enabled && (keyed || response.secondary_clicked()) {
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
    let mut items = Vec::new();
    let mut actions: Vec<Action> = Vec::new();
    if looper.can_redo {
        actions.push(Command::LoopRecorderRedo.into());
        items.push(texts.get("loop_recorder.redo"));
    }
    actions.push(Command::LoopRecorderDelete.into());
    items.push(texts.get("loop_recorder.delete"));
    let items: Vec<Item<'_>> = items
        .into_iter()
        .zip(actions)
        .map(|(label, action)| Item {
            label,
            action: Some(action),
        })
        .collect();
    let outcome = if enabled && cx.interactive {
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
