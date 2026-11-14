//! A deck: its header, the overview of its track, the transport keys and the tempo control.
//!
//! Components: `DeckHeader`, `TransportButton`, `TempoControl` and `Notice` in
//! `docs/design/DESIGN.md`; placement from `docs/design/screens/M2-main.html`. Behaviour
//! note: `docs/behavior/transport.md`.

use std::path::PathBuf;

use egui::{Align2, CornerRadius, Rect, Role, Sense, StrokeKind, WidgetInfo};
use platter_core::DeckId;
use platter_engine::{Command, DeckSnapshot};

use crate::command::AppCommand;
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::icons::Glyph;
use crate::model::{DeckNotice, View};
use crate::paint;
use crate::panels::waveform::{self, deck_color};
use crate::shell::ViewState;
use crate::widgets::key::{self, Face, KeySpec, Press};
use crate::widgets::value::{self, FADER_WIDTH, FaderSpec, Steps, ValueSpec};

/// How much the bend keys change the speed while held.
pub const NUDGE_AMOUNT: f64 = 0.04;
/// Seconds left below which the remaining time turns to the warning colour.
const ENDING_SECONDS: f64 = 30.0;

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
    if let Some(notice) = view.notices[deck.index()] {
        let area = Rect::from_min_size(left.min, egui::vec2(left.width(), 52.0));
        deck_notice(cx, area, deck, notice);
    }
    transport(cx, left, deck, snapshot);
    tempo(cx, tempo_column, deck, snapshot);
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
    let base = track.and_then(|track| track.tags.bpm).filter(|_| loaded);
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
