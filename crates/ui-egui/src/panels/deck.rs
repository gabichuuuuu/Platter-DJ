//! A deck: its header, the overview of its track, the row of sync, the hotcues, the loop
//! controls, a page of the advanced panel, the transport keys and the tempo control.
//!
//! Components: `DeckHeader`, `TransportButton`, `TempoControl`, `SyncControls`, `HotcuePads`,
//! `LoopControls`, `MovePanel`, `CuePanel`, `GridEditor` and `Notice` in
//! `docs/design/DESIGN.md`; placement from `docs/design/screens/M2-main.html` and, for the
//! overview, the rows of sync and loops and the pages, `docs/design/screens/First-version.html`.
//! Behaviour notes: `docs/behavior/transport.md`, `docs/behavior/beatgrid.md`,
//! `docs/behavior/loops.md`, `docs/behavior/cues.md` and `docs/behavior/sync.md`.

use std::path::PathBuf;

use egui::{
    Align2, Color32, CornerRadius, Key, Rect, Role, Sense, Shape, Stroke, StrokeKind, WidgetInfo,
};
use platter_core::{Bpm, Cue, CueKind, DeckId, HotcueSlot};
use platter_engine::{
    BpmScale, Command, CueStep, DeckSnapshot, Direction, MoveMode, MoveSize, SizeStep,
};

use crate::command::{Action, AppCommand, DeckPage, DeckSize};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::i18n::Texts;
use crate::icons::Glyph;
use crate::keys;
use crate::model::{DeckNotice, View};
use crate::paint;
use crate::panels::browser;
use crate::panels::waveform::{self, deck_color};
use crate::shell::ViewState;
use crate::tokens::{TextStyle, Tokens, Weight};
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::field::{self, Typing};
use crate::widgets::key::{self, Face, KeySpec, Press};
use crate::widgets::value::{self, FADER_WIDTH, FaderSpec, Steps, TEMPO_FADER, ValueSpec};

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
/// Height of the overview of the track, as `First-version.html` draws it.
const OVERVIEW_HEIGHT: f32 = 36.0;
const SYNC_WIDTH: f32 = 56.0;
const MASTER_WIDTH: f32 = 68.0;
const PHASE_WIDTH: f32 = 120.0;
const PHASE_HEIGHT: f32 = 6.0;
/// The key lock and the semitone keys, as the reference screen draws them.
const KEY_LOCK_WIDTH: f32 = 28.0;
const KEY_STEP_WIDTH: f32 = 24.0;
const KEY_READOUT_WIDTH: f32 = 44.0;
const KEY_ROW_HEIGHT: f32 = 24.0;
const KEY_GAP: f32 = 4.0;
/// The cover in the deck header, as the reference screen draws it.
const COVER_SIDE: f32 = 54.0;
/// The height of the header of a full deck, and of a compact one.
const HEADER_HEIGHT: f32 = 56.0;
const COMPACT_HEADER_HEIGHT: f32 = 26.0;
/// What a compact deck draws under its overview, the advanced panel left out.
const COMPACT_CONTROLS_HEIGHT: f32 = 192.0;
/// FLUX and REVERSE beside play, as the reference screen draws them.
const FLUX_WIDTH: f32 = 52.0;
const REVERSE_WIDTH: f32 = 36.0;
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
    drop_target(cx, rect, deck);

    let compact = view.settings.deck_size == DeckSize::Compact;
    let overview = if compact {
        compact_header(
            cx,
            Rect::from_min_size(inner.min, egui::vec2(inner.width(), COMPACT_HEADER_HEIGHT)),
            deck,
            view,
            state,
        );
        // The overview takes what the band leaves over.
        let top = inner.top() + COMPACT_HEADER_HEIGHT + 10.0;
        let height = (inner.bottom() - COMPACT_CONTROLS_HEIGHT - 10.0 - top).max(OVERVIEW_HEIGHT);
        Rect::from_min_size(
            egui::pos2(inner.left(), top),
            egui::vec2(inner.width(), height),
        )
    } else {
        header(
            cx,
            Rect::from_min_size(inner.min, egui::vec2(inner.width(), HEADER_HEIGHT)),
            deck,
            view,
            state,
        );
        Rect::from_min_size(
            inner.min + egui::vec2(0.0, HEADER_HEIGHT + 10.0),
            egui::vec2(inner.width(), OVERVIEW_HEIGHT),
        )
    };
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
    sync_controls(cx, left, deck, view, false);
    // A notice takes the place of the hotcues for as long as it shows.
    if let Some(notice) = view.notices[deck.index()] {
        let top = left.top() + tokens.size.button_h + tokens.space.s3;
        let area =
            Rect::from_min_size(egui::pos2(left.left(), top), egui::vec2(left.width(), 52.0));
        deck_notice(cx, area, deck, notice);
    }
    // Without the advanced panel the hotcues and the loop row sit lower, where its rows
    // would be: they are placed from the bottom of an area that reaches past the deck by
    // the height of those rows.
    let above_pages = if compact {
        Rect::from_min_max(left.min, left.max + egui::vec2(0.0, pages_height(tokens)))
    } else {
        left
    };
    if view.notices[deck.index()].is_none() {
        hotcue_pads(
            cx,
            &PadGrid::above_loop(above_pages, tokens),
            deck,
            snapshot,
            mapping,
        );
    }
    let transport_row = Rect::from_min_max(
        egui::pos2(left.left(), left.bottom() - tokens.size.transport_h),
        left.max,
    );
    transport(
        cx,
        transport_row,
        tokens.size.button_transport,
        deck,
        snapshot,
    );
    loop_controls(cx, above_pages, deck, snapshot);
    if !compact {
        let page = state.deck_page[deck.index()];
        match page {
            DeckPage::Move => move_page(cx, left, deck, snapshot),
            DeckPage::Cue => cue_page(cx, left, deck, snapshot, mapping),
            DeckPage::Grid => grid_page(cx, left, deck, snapshot),
        }
        page_selector(cx, left, deck, page);
    }
    tempo(cx, tempo_column, deck, snapshot);
}

/// A track dragged from the file list and released on the deck is loaded into it.
fn drop_target(cx: &mut Cx<'_>, rect: Rect, deck: DeckId) {
    let theme = cx.theme;
    let drop = cx
        .ui
        .interact(rect, id(("deck-drop", deck.index())), Sense::hover());
    if !cx.interactive {
        return;
    }
    if drop.dnd_hover_payload::<PathBuf>().is_some() {
        cx.painter().rect_stroke(
            rect,
            CornerRadius::ZERO,
            paint::hairline(theme, theme.tokens.color.text_primary),
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

/// Padding of a deck in the four-deck layout and the sizes of its rows:
/// `docs/design/DESIGN.md`, *Four-deck layout (M4)*.
const FOUR_PADDING: f32 = 10.0;
const FOUR_HEADER_HEIGHT: f32 = 40.0;
const FOUR_LANE_HEIGHT: f32 = 64.0;
const FOUR_STRIPE_HEIGHT: f32 = 18.0;
const FOUR_ROW_HEIGHT: f32 = 30.0;
const FOUR_PAD_HEIGHT: f32 = 26.0;
const FOUR_TEMPO_WIDTH: f32 = 48.0;
const FOUR_KEY_WIDTH: f32 = 40.0;
const FOUR_COVER_SIDE: f32 = 38.0;
const FOUR_LOOP_SIZE_WIDTH: f32 = 30.0;
const FOUR_LOOP_KEY_WIDTH: f32 = 32.0;
const NARROW_REVERSE_WIDTH: f32 = 28.0;
const NARROW_PLAY_WIDTH: f32 = 44.0;
const NARROW_ARROW_WIDTH: f32 = 26.0;
const NARROW_SYNC_WIDTH: f32 = 50.0;
const NARROW_MASTER_WIDTH: f32 = 64.0;
const NARROW_TEMPO_KEY_HEIGHT: f32 = 22.0;
const FOUR_TITLE_WIDTH: f32 = 180.0;
const FOUR_TAG_WIDTH: f32 = 42.0;

/// A deck of the four-deck layout: a header of two lines, the scrolling waveform with the
/// overview under it, the row of sync, the transport and loop row and the eight hotcues in one
/// row, with the tempo column at the right. No advanced panel and no beatjump.
/// `docs/behavior/interface.md`, *Deck layout*.
pub fn four_deck(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let snapshot = &view.engine.decks[deck.index()];
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, tokens.color.surface_panel);
    let inner = rect.shrink(FOUR_PADDING);
    drop_target(cx, rect, deck);

    four_header(
        cx,
        Rect::from_min_size(inner.min, egui::vec2(inner.width(), FOUR_HEADER_HEIGHT)),
        deck,
        view,
        state,
    );
    let body = Rect::from_min_max(
        egui::pos2(
            inner.left(),
            inner.top() + FOUR_HEADER_HEIGHT + FOUR_PADDING,
        ),
        inner.max,
    );
    let tempo_column = Rect::from_min_max(
        egui::pos2(body.right() - FOUR_TEMPO_WIDTH, body.top()),
        body.max,
    );
    let main = Rect::from_min_max(
        body.min,
        egui::pos2(tempo_column.left() - FOUR_PADDING, body.bottom()),
    );
    let lane = Rect::from_min_size(main.min, egui::vec2(main.width(), FOUR_LANE_HEIGHT));
    waveform::deck_lane(cx, lane, deck, snapshot, state.zoom[deck.index()]);
    let stripe = Rect::from_min_size(
        egui::pos2(
            main.left(),
            lane.bottom() + tokens.space.s2 + tokens.space.s1,
        ),
        egui::vec2(main.width(), FOUR_STRIPE_HEIGHT),
    );
    waveform::overview(cx, stripe, deck, snapshot);

    // From the bottom up: the hotcues, the transport and loop row, then the row of sync.
    let pads = Rect::from_min_max(
        egui::pos2(main.left(), main.bottom() - FOUR_PAD_HEIGHT),
        main.max,
    );
    let row = Rect::from_min_size(
        egui::pos2(
            main.left(),
            pads.top() - tokens.space.s5 - tokens.space.s1 - FOUR_ROW_HEIGHT,
        ),
        egui::vec2(main.width(), FOUR_ROW_HEIGHT),
    );
    let sync_row = Rect::from_min_max(
        egui::pos2(
            main.left(),
            stripe.bottom() + tokens.space.s5 + tokens.space.s1,
        ),
        egui::pos2(main.right(), row.top()),
    );
    sync_controls(cx, sync_row, deck, view, true);
    // A notice takes the place of the transport and the hotcues for as long as it shows.
    if let Some(notice) = view.notices[deck.index()] {
        deck_notice(cx, Rect::from_min_max(row.min, pads.max), deck, notice);
    } else {
        hotcue_pads(
            cx,
            &PadGrid {
                area: pads,
                columns: HotcueSlot::ALL.len(),
                height: FOUR_PAD_HEIGHT,
            },
            deck,
            snapshot,
            state.cue_map[deck.index()],
        );
        let end = transport(cx, row, FOUR_KEY_WIDTH, deck, snapshot);
        let gap = tokens.space.s2;
        // The loop keys end at the right of the row when they fit there: the size between its
        // arrows, In, Out and the wider Active.
        let width = 2.0 * NARROW_ARROW_WIDTH
            + FOUR_LOOP_SIZE_WIDTH
            + tokens.space.s3
            + 2.0 * FOUR_LOOP_KEY_WIDTH
            + 52.0
            + 5.0 * gap;
        loop_keys(
            cx,
            Row {
                x: (row.right() - width).max(end + tokens.space.s3),
                top: row.top(),
                height: row.height(),
                gap,
            },
            (FOUR_LOOP_SIZE_WIDTH, FOUR_LOOP_KEY_WIDTH),
            deck,
            snapshot,
        );
    }
    tempo(cx, tempo_column, deck, snapshot);
}

/// The header of a deck of the four-deck layout: the letter, the cover, the title over the
/// artist and album, the tempo over its offset and key, the large time over the next one.
fn four_header(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let snapshot = &view.engine.decks[deck.index()];
    let loaded = is_ready(snapshot);
    let badge = Rect::from_min_size(rect.min, egui::vec2(22.0, 22.0));
    paint::badge(
        cx.painter(),
        theme,
        badge,
        deck.letter(),
        deck_color(theme, deck),
    );
    let cover = Rect::from_min_size(
        egui::pos2(badge.right() + FOUR_PADDING, rect.top()),
        egui::vec2(FOUR_COVER_SIDE, FOUR_COVER_SIDE),
    );
    paint_cover(cx, cover, deck, view);
    let time_block = Rect::from_min_max(egui::pos2(rect.right() - 92.0, rect.top()), rect.max);
    let tempo_block = Rect::from_min_max(
        egui::pos2(time_block.left() - FOUR_PADDING - 64.0, rect.top()),
        egui::pos2(time_block.left() - FOUR_PADDING, rect.bottom()),
    );
    let text_block = Rect::from_min_max(
        egui::pos2(cover.right() + FOUR_PADDING, rect.top()),
        egui::pos2(tempo_block.left() - FOUR_PADDING, rect.bottom()),
    );
    let line = |block: Rect, top: f32, height: f32| {
        Rect::from_min_size(
            egui::pos2(block.left(), block.top() + top),
            egui::vec2(block.width(), height),
        )
    };
    let title_line = line(text_block, 0.0, 18.0);
    match title_of(snapshot) {
        Some(title) => {
            // The title takes what its words need, up to its width, and the tag of the kind of
            // deck follows it.
            let width = paint::text_width(cx.painter(), theme, &title, tokens.text.title)
                .min(FOUR_TITLE_WIDTH)
                .min(title_line.width() - FOUR_TAG_WIDTH - tokens.space.s3);
            let title_line = Rect::from_min_size(title_line.min, egui::vec2(width, 18.0));
            let tag = Rect::from_min_size(
                egui::pos2(title_line.right() + tokens.space.s3, title_line.top() + 1.0),
                egui::vec2(FOUR_TAG_WIDTH, 16.0),
            );
            paint::tag(cx.painter(), theme, tag, texts.get("deck.kind_track"));
            let elided = paint::text_elided(
                cx.painter(),
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
            let tags = snapshot.track.as_ref().map(|track| &track.tags);
            let artist = tags
                .and_then(|tags| tags.artist.as_deref())
                .filter(|artist| !artist.is_empty());
            let album = tags
                .and_then(|tags| tags.album.as_deref())
                .filter(|album| !album.is_empty());
            let second = match (snapshot.loading, artist, album) {
                (true, _, _) => texts.get("deck.loading").to_owned(),
                (false, Some(artist), Some(album)) => {
                    texts.format("deck.artist_album", &[("artist", artist), ("album", album)])
                }
                (false, artist, album) => artist.or(album).unwrap_or_default().to_owned(),
            };
            paint::text_elided(
                cx.painter(),
                theme,
                line(text_block, 18.0, 18.0),
                &second,
                tokens.text.body,
                color.text_secondary,
            );
        }
        None => {
            let waiting = texts.get(if snapshot.loading {
                "deck.loading"
            } else {
                "deck.empty_title"
            });
            paint::text_elided(
                cx.painter(),
                theme,
                title_line,
                waiting,
                tokens.text.title,
                color.text_secondary,
            );
        }
    }

    let readings = Readings::of(cx, snapshot, state.remaining_large[deck.index()]);
    // Under the tempo: its offset, then the key the deck plays when there is one.
    let offset = format::signed_percent(snapshot.speed - 1.0);
    let detail = match played_key(texts, snapshot, view).filter(|_| loaded) {
        Some(key) => texts.format("deck.offset_key", &[("offset", &offset), ("key", &key)]),
        None => texts.format("deck.tempo_offset", &[("offset", &offset)]),
    };
    let painter = cx.painter();
    paint::text(
        painter,
        theme,
        line(tempo_block, 0.0, 22.0),
        Align2::RIGHT_CENTER,
        &readings.tempo,
        tokens.text.tempo,
        readings.tempo_ink,
    );
    paint::text(
        painter,
        theme,
        line(tempo_block, 22.0, 14.0),
        Align2::RIGHT_CENTER,
        &detail,
        tokens.text.readout,
        readings.small_ink,
    );
    time_key(cx, time_block, deck, loaded);
    let painter = cx.painter();
    paint::text(
        painter,
        theme,
        line(time_block, 0.0, 26.0),
        Align2::RIGHT_CENTER,
        &readings.large,
        tokens.text.time,
        readings.large_ink,
    );
    paint::text(
        painter,
        theme,
        line(time_block, 26.0, 14.0),
        Align2::RIGHT_CENTER,
        &readings.second,
        tokens.text.readout,
        readings.small_ink,
    );
}

/// The row of sync at the top of `area`: SYNC, MASTER, the phase meter and, beside it on the
/// deck that is the tempo master, its caption.
fn sync_controls(cx: &mut Cx<'_>, area: Rect, deck: DeckId, view: &View<'_>, narrow: bool) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let snapshot = &view.engine.decks[deck.index()];
    let clock = &view.engine.clock;
    let lit = deck_color(theme, deck);
    let letter = deck.letter().to_string();
    let master = clock.master == Some(deck);
    // The row of a deck of the four-deck layout is lower and its keys narrower.
    let (height, gap, sync_width, master_width) = if narrow {
        (
            KEY_ROW_HEIGHT,
            tokens.space.s2 + tokens.space.s1,
            NARROW_SYNC_WIDTH,
            NARROW_MASTER_WIDTH,
        )
    } else {
        (
            tokens.size.button_h,
            tokens.space.s3,
            SYNC_WIDTH,
            MASTER_WIDTH,
        )
    };
    let mut row = Row {
        x: area.left(),
        top: area.top(),
        height,
        gap,
    };

    let label = texts.get("deck.sync");
    let rect = row.place(label_width(cx, label, sync_width, narrow));
    let name = texts.format("deck.sync_name", &[("deck", &letter)]);
    let mut spec = KeySpec::text(label).lit((snapshot.sync && !master).then_some(lit));
    spec.name = &name;
    key::key(
        cx,
        rect,
        id(("deck-sync", deck.index())),
        spec,
        Press::Click(Command::DeckSyncToggle { deck }.into()),
    );

    // In the automatic mode the engine chooses the tempo master and the key only shows it.
    let label = texts.get("deck.master");
    let rect = row.place(label_width(cx, label, master_width, narrow));
    let name = texts.format("deck.master_name", &[("deck", &letter)]);
    let has_tempo = is_ready(snapshot) && snapshot.grid.bpm().is_some();
    let mut spec = KeySpec::text(label)
        .lit(master.then_some(lit))
        .enabled(!clock.auto && has_tempo);
    spec.name = &name;
    spec.keeps_led = true;
    key::key(
        cx,
        rect,
        id(("deck-master", deck.index())),
        spec,
        Press::Click(Command::DeckMasterSet { deck }.into()),
    );

    let slot = Rect::from_min_size(
        egui::pos2(row.x, area.top() + (height - PHASE_HEIGHT) / 2.0),
        egui::vec2(PHASE_WIDTH, PHASE_HEIGHT),
    );
    let painter = cx.painter();
    painter.rect(
        slot,
        paint::radius(tokens.radius.sm),
        color.bg_app,
        paint::hairline(theme, color.line_key),
        StrokeKind::Inside,
    );
    // Half a beat behind fills the left half, half a beat ahead the right half.
    if let Some(phase) = snapshot.phase.filter(|phase| phase.is_finite()) {
        let reach = phase.clamp(-0.5, 0.5) * PHASE_WIDTH;
        if reach.abs() >= 1.0 {
            let centre = slot.center().x;
            let bar = Rect::from_x_y_ranges(
                centre.min(centre + reach)..=centre.max(centre + reach),
                slot.top() + 1.0..=slot.bottom() - 1.0,
            );
            painter.rect_filled(bar, CornerRadius::ZERO, lit);
        }
    }
    let centre = slot.center().x;
    painter.vline(
        centre,
        slot.top() - 4.0..=slot.bottom() + 4.0,
        paint::hairline(theme, color.line_control),
    );
    let name = texts.format("deck.phase_name", &[("deck", &letter)]);
    let response = cx
        .ui
        .interact(slot, id(("deck-phase", deck.index())), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Image, true, &name));
    let keys_left = key_controls(cx, area, height, deck, view);
    // The narrow row has no room for the caption: the lit MASTER says it.
    if master && !narrow {
        let caption = Rect::from_min_max(
            egui::pos2(slot.right() + gap, area.top()),
            egui::pos2(keys_left - gap, area.top() + height),
        );
        paint::text_elided(
            cx.painter(),
            theme,
            caption,
            texts.get("deck.tempo_master"),
            tokens.text.label,
            color.text_secondary,
        );
    }
}

/// Key lock, a semitone down, the key the deck plays and a semitone up, at the right of the
/// row of sync. Returns where they start.
fn key_controls(
    cx: &mut Cx<'_>,
    area: Rect,
    row_height: f32,
    deck: DeckId,
    view: &View<'_>,
) -> f32 {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let snapshot = &view.engine.decks[deck.index()];
    let letter = deck.letter().to_string();
    let ready = is_ready(snapshot);
    let top = area.top() + (row_height - KEY_ROW_HEIGHT) / 2.0;
    let width = KEY_LOCK_WIDTH + 2.0 * KEY_STEP_WIDTH + KEY_READOUT_WIDTH + 3.0 * KEY_GAP;
    let left = area.right() - width;
    let mut row = Row {
        x: left,
        top,
        height: KEY_ROW_HEIGHT,
        gap: KEY_GAP,
    };

    let name = texts.format("deck.key_lock_name", &[("deck", &letter)]);
    let spec = KeySpec::icon(Glyph::Lock, 14.0, &name)
        .lit(snapshot.key_lock.then_some(color.text_primary))
        .enabled(ready);
    key::key(
        cx,
        row.place(KEY_LOCK_WIDTH),
        id(("deck-key-lock", deck.index())),
        spec,
        Press::Click(Command::DeckKeyLockToggle { deck }.into()),
    );
    let down = texts.format("deck.key_down_name", &[("deck", &letter)]);
    let up = texts.format("deck.key_up_name", &[("deck", &letter)]);
    let shift = snapshot.key_shift;
    let step = |cx: &mut Cx<'_>, rect: Rect, glyph: Glyph, name: &str, direction: SizeStep| {
        let limit = if direction == SizeStep::Down {
            -12.0
        } else {
            12.0
        };
        key::key(
            cx,
            rect,
            id(("deck-key-step", deck.index(), direction == SizeStep::Up)),
            KeySpec::icon(glyph, 14.0, name).enabled(ready && shift != limit),
            Press::Click(Command::DeckKeyStep { deck, direction }.into()),
        );
    };
    step(
        cx,
        row.place(KEY_STEP_WIDTH),
        Glyph::Minus,
        &down,
        SizeStep::Down,
    );
    let readout = row.place(KEY_READOUT_WIDTH);
    step(
        cx,
        row.place(KEY_STEP_WIDTH),
        Glyph::Plus,
        &up,
        SizeStep::Up,
    );
    let played = played_key(texts, snapshot, view).unwrap_or_else(|| "--".to_owned());
    paint::text(
        cx.painter(),
        theme,
        readout,
        Align2::CENTER_CENTER,
        &played,
        tokens.text.label,
        if ready {
            color.text_primary
        } else {
            color.text_disabled
        },
    );
    left
}

/// The key the deck plays: what the analysis found, moved by the shift or the speed.
fn played_key(texts: &Texts, snapshot: &DeckSnapshot, view: &View<'_>) -> Option<String> {
    snapshot.resulting_key.map(|transposed| {
        keys::key_text(
            keys::number_of(transposed.key),
            view.settings.key_notation,
            (
                texts.get("browser.key_major"),
                texts.get("browser.key_minor"),
            ),
        )
    })
}

/// The tops of the two rows of a page of the advanced panel, above the transport keys at
/// the bottom of `area`.
/// The height of the two rows of the advanced panel, with the gaps around them.
fn pages_height(tokens: &Tokens) -> f32 {
    2.0 * GRID_ROW_HEIGHT + tokens.space.s2 + tokens.space.s3
}

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
    stepper_sized(cx, row, salt, reading, arrows, ARROW_WIDTH);
}

/// A stepper whose arrows are `arrow_width` wide.
fn stepper_sized(
    cx: &mut Cx<'_>,
    row: &mut Row,
    salt: (&'static str, usize),
    reading: (&str, TextStyle, f32),
    arrows: [Arrow<'_>; 2],
    arrow_width: f32,
) {
    let color = &cx.theme.tokens.color;
    let (text, style, width) = reading;
    let [down, up] = arrows;
    let active = down.enabled || up.enabled;
    let rect = row.place(arrow_width);
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
    let rect = row.place(arrow_width);
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
    let loaded = is_ready(snapshot);
    let (first_top, _) = page_rows(area, tokens);
    let top = first_top - tokens.space.s3 - LOOP_ROW_HEIGHT;
    let gap = tokens.space.s2;
    loop_keys(
        cx,
        Row {
            x: area.left(),
            top,
            height: LOOP_ROW_HEIGHT,
            gap,
        },
        (LOOP_SIZE_WIDTH, 36.0),
        deck,
        snapshot,
    );

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

/// The loop size between its arrows, then In, Out and Active, from the left of `row`. The size
/// reading is `size_width` wide and each key at least `key_width`.
fn loop_keys(
    cx: &mut Cx<'_>,
    mut row: Row,
    (size_width, key_width): (f32, f32),
    deck: DeckId,
    snapshot: &DeckSnapshot,
) {
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
    let size = controls.loop_size;
    let size_text = waveform::size_text(texts, size);
    let smaller = named("deck.loop_smaller_name");
    let larger = named("deck.loop_larger_name");
    let step = |direction| Command::DeckLoopSizeStep { deck, direction }.into();
    let arrow_width = if size_width < LOOP_SIZE_WIDTH {
        NARROW_ARROW_WIDTH
    } else {
        ARROW_WIDTH
    };
    stepper_sized(
        cx,
        &mut row,
        ("deck-loop-size", deck.index()),
        (&size_text, tokens.text.tempo, size_width),
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
        arrow_width,
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
        let rect = row.place(label_width(cx, label, key_width, key_width < 36.0));
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
/// Where the eight hotcue pads go: `area` is split in `columns` columns and as many rows as the
/// pads need, each pad `height` tall.
struct PadGrid {
    area: Rect,
    columns: usize,
    height: f32,
}

impl PadGrid {
    /// Two rows of four, above the loop row of the full and compact decks.
    fn above_loop(area: Rect, tokens: &Tokens) -> Self {
        let gap = tokens.space.s2;
        let (first_top, _) = page_rows(area, tokens);
        let bottom = first_top - tokens.space.s3 - LOOP_ROW_HEIGHT - gap;
        let top = bottom - 2.0 * PAD_HEIGHT - gap;
        Self {
            area: Rect::from_min_max(
                egui::pos2(area.left(), top),
                egui::pos2(area.right(), bottom),
            ),
            columns: PAD_COLUMNS,
            height: PAD_HEIGHT,
        }
    }
}

fn hotcue_pads(
    cx: &mut Cx<'_>,
    grid: &PadGrid,
    deck: DeckId,
    snapshot: &DeckSnapshot,
    mapping: bool,
) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let letter = deck.letter().to_string();
    let loaded = is_ready(snapshot);
    let gap = tokens.space.s2;
    let area = grid.area;
    let top = area.top();
    let columns = grid.columns.max(1);
    let width = (area.width() - (columns as f32 - 1.0) * gap) / columns as f32;
    let mut bold = tokens.text.button;
    bold.weight = Weight::Bold;
    for slot in HotcueSlot::ALL {
        let index = slot.index();
        let rect = Rect::from_min_size(
            egui::pos2(
                area.left() + (index % columns) as f32 * (width + gap),
                top + (index / columns) as f32 * (grid.height + gap),
            ),
            egui::vec2(width, grid.height),
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
            // The name is typed after a double click, or Enter while it has the keyboard.
            let name_id = id(("deck-cue-name", deck.index()));
            let rename = named("deck.cue_rename_name");
            match field::typing(
                cx,
                label,
                name_id,
                cue.name.as_str(),
                &rename,
                "deck.cue.rename",
            ) {
                Typing::Idle => {
                    cx.record("deck.cue.rename", label, ControlKind::Keys, true);
                    let response = cx
                        .ui
                        .interact(label, name_id, cx.sense(true, Sense::click()));
                    response.widget_info(|| WidgetInfo::labeled(Role::TextInput, true, &rename));
                    if response.clicked() {
                        response.request_focus();
                    }
                    let painter = cx.painter();
                    paint::text_elided(
                        painter,
                        theme,
                        label,
                        cue_label(texts, cue),
                        tokens.text.body,
                        color.text_primary,
                    );
                    if response.has_focus() {
                        paint::focus_ring(painter, theme, label, tokens.radius.sm);
                    }
                }
                Typing::Open => {}
                Typing::Confirmed(name) => cx.out.emit(Command::DeckCueRename { deck, name }),
            }
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
    let typed = if editable {
        field::typing(
            cx,
            field_rect,
            id(("deck-grid-bpm", deck.index())),
            &format::bpm(start),
            &name,
            "deck.grid.bpm.set",
        )
    } else {
        Typing::Idle
    };
    match typed {
        Typing::Idle => {
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
        }
        Typing::Open => {}
        Typing::Confirmed(text) => {
            if let Some(bpm) = format::typed_bpm(&text) {
                cx.out.emit(Command::DeckGridBpmSet { deck, bpm });
            }
        }
    }

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
    // The beat tick, just before the lock.
    let tick = Rect::from_min_size(
        egui::pos2(lock.left() - gap - GRID_KEY_WIDTH, first_top),
        egui::vec2(GRID_KEY_WIDTH, height),
    );
    let name = named("deck.beat_tick_name");
    let spec = KeySpec::icon(Glyph::Metronome, 14.0, &name)
        .enabled(loaded)
        .lit(snapshot.beat_tick.then_some(deck_color(theme, deck)));
    key::key(
        cx,
        tick,
        id(("deck-beat-tick", deck.index())),
        spec,
        Press::Click(Command::DeckBeatTickToggle { deck }.into()),
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
    // The resets bring back what the analysis of the load found: the grid, or its tempo alone.
    let found = snapshot
        .analysis
        .and_then(|analysis| analysis.tempo)
        .is_some();
    let analysed = snapshot.analysis.is_some();
    let resets = [
        (
            Glyph::Grid,
            "deck.grid_reset_name",
            Command::DeckGridReset { deck },
            editable && analysed,
        ),
        (
            Glyph::Clock,
            "deck.grid_bpm_reset_name",
            Command::DeckGridBpmReset { deck },
            editable && found,
        ),
    ];
    for (index, (glyph, name, command, enabled)) in resets.into_iter().enumerate() {
        let rect = row.place(GRID_KEY_WIDTH);
        let name = named(name);
        key::key(
            cx,
            rect,
            id(("deck-grid-reset", deck.index(), index)),
            KeySpec::icon(glyph, 14.0, &name).enabled(enabled),
            Press::Click(command.into()),
        );
    }
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
    // The cover of the track, or a frame with the icon of an audio file.
    let cover = Rect::from_min_size(
        egui::pos2(badge.right() + tokens.space.s3, rect.top()),
        egui::vec2(COVER_SIDE, COVER_SIDE),
    );
    paint_cover(cx, cover, deck, view);
    let text_block = Rect::from_min_max(
        egui::pos2(cover.right() + tokens.space.s4, rect.top()),
        egui::pos2(tempo_block.left() - tokens.space.s4, rect.bottom()),
    );
    size_switch(cx, Rect::from_min_max(rect.min, text_block.max), deck, view);
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

    let readings = Readings::of(cx, snapshot, state.remaining_large[deck.index()]);
    paint::text(
        painter,
        theme,
        line(tempo_block, 0.0, 22.0),
        Align2::RIGHT_CENTER,
        &readings.tempo,
        tokens.text.tempo,
        readings.tempo_ink,
    );
    paint::text(
        painter,
        theme,
        line(tempo_block, 22.0, 14.0),
        Align2::RIGHT_CENTER,
        &readings.detail,
        tokens.text.readout,
        readings.small_ink,
    );

    // Times: one large, the other two below it. A click swaps remaining and elapsed.
    time_key(cx, time_block, deck, loaded);
    paint::text(
        painter,
        theme,
        line(time_block, 0.0, 26.0),
        Align2::RIGHT_CENTER,
        &readings.large,
        tokens.text.time,
        readings.large_ink,
    );
    for (index, text) in [&readings.second, &readings.third].iter().enumerate() {
        paint::text(
            painter,
            theme,
            line(time_block, 26.0 + 14.0 * index as f32, 14.0),
            Align2::RIGHT_CENTER,
            text,
            tokens.text.readout,
            readings.small_ink,
        );
    }
}

/// The cover of the track of a deck in `rect`, or a frame with the icon of an audio file.
fn paint_cover(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let snapshot = &view.engine.decks[deck.index()];
    cx.painter().rect(
        rect,
        paint::radius(tokens.radius.sm),
        color.bg_app,
        paint::hairline(theme, color.line_key),
        StrokeKind::Inside,
    );
    let image = snapshot
        .track
        .as_ref()
        .zip(view.browser.deck_covers[deck.index()].as_ref());
    match image {
        Some((track, image)) => {
            let texture = browser::cover_texture(cx, &track.path, image);
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            cx.painter()
                .image(texture, rect.shrink(1.0), uv, egui::Color32::WHITE);
        }
        None => {
            // The icon keeps the proportion it has in the cover of the full header.
            let side = (rect.width() * 26.0 / COVER_SIDE).round();
            theme.icons.paint(
                cx.painter(),
                Glyph::FileAudio,
                Rect::from_center_size(rect.center(), egui::vec2(side, side)),
                color.text_disabled,
            );
        }
    }
}

/// The header of a compact deck: one row with the letter, the title, the current tempo and
/// the large time. `docs/behavior/interface.md`, *Deck sizes*.
fn compact_header(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let snapshot = &view.engine.decks[deck.index()];
    let loaded = is_ready(snapshot);
    let badge = Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - 11.0),
        egui::vec2(22.0, 22.0),
    );
    paint::badge(
        painter,
        theme,
        badge,
        deck.letter(),
        deck_color(theme, deck),
    );
    let time_block = Rect::from_min_max(egui::pos2(rect.right() - 96.0, rect.top()), rect.max);
    let tempo_block = Rect::from_min_max(
        egui::pos2(time_block.left() - tokens.space.s4 - 96.0, rect.top()),
        egui::pos2(time_block.left() - tokens.space.s4, rect.bottom()),
    );
    let text_block = Rect::from_min_max(
        egui::pos2(badge.right() + tokens.space.s4, rect.top()),
        egui::pos2(tempo_block.left() - tokens.space.s4, rect.bottom()),
    );
    size_switch(cx, Rect::from_min_max(rect.min, text_block.max), deck, view);

    let title_line =
        Rect::from_center_size(text_block.center(), egui::vec2(text_block.width(), 18.0));
    match title_of(snapshot) {
        Some(title) => {
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
        }
        None => {
            let waiting = texts.get(if snapshot.loading {
                "deck.loading"
            } else {
                "deck.empty_title"
            });
            paint::text_elided(
                painter,
                theme,
                title_line,
                waiting,
                tokens.text.title,
                color.text_secondary,
            );
        }
    }

    let readings = Readings::of(cx, snapshot, state.remaining_large[deck.index()]);
    paint::text(
        painter,
        theme,
        tempo_block,
        Align2::RIGHT_CENTER,
        &readings.tempo,
        tokens.text.tempo,
        readings.tempo_ink,
    );
    time_key(cx, time_block, deck, loaded);
    paint::text(
        painter,
        theme,
        time_block,
        Align2::RIGHT_CENTER,
        &readings.large,
        tokens.text.time,
        readings.large_ink,
    );
}

/// A double click on the header of a deck, left of its tempo, switches the size of the decks.
/// The pointer alone operates it; the General page of the preferences sends the same command.
fn size_switch(cx: &mut Cx<'_>, area: Rect, deck: DeckId, view: &View<'_>) {
    let response = cx.ui.interact(
        area,
        id(("deck-size", deck.index())),
        cx.sense(true, Sense::click()),
    );
    if response.double_clicked() {
        cx.out.emit(AppCommand::DeckSizeSet {
            size: view.settings.deck_size.other(),
        });
    }
}

/// The key the times of a deck are: a click swaps remaining and elapsed.
fn time_key(cx: &mut Cx<'_>, time_block: Rect, deck: DeckId, loaded: bool) {
    let theme = cx.theme;
    let letter = deck.letter().to_string();
    let action = AppCommand::DeckTimeDisplayToggle { deck };
    cx.record(action.id(), time_block, ControlKind::Button, loaded);
    let response = cx.ui.interact(
        time_block,
        id(("deck-time", deck.index())),
        cx.sense(loaded, Sense::click()),
    );
    let name = cx.texts.format("deck.time_name", &[("deck", &letter)]);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, loaded, &name));
    if response.has_focus() {
        cx.out.claim(&[egui::Key::Space, egui::Key::Enter]);
        paint::focus_ring(cx.painter(), theme, time_block, theme.tokens.radius.md);
    }
    if response.clicked() {
        cx.out.emit(action);
    }
}

/// What the header of a deck reads out: the tempo and the times, with their colours.
struct Readings {
    tempo: String,
    /// The offset of the tempo, and the base tempo when there is one.
    detail: String,
    tempo_ink: Color32,
    large: String,
    second: String,
    third: String,
    large_ink: Color32,
    /// The colour of the small readings.
    small_ink: Color32,
}

impl Readings {
    fn of(cx: &Cx<'_>, snapshot: &DeckSnapshot, remaining_large: bool) -> Self {
        let texts = cx.texts;
        let color = &cx.theme.tokens.color;
        let track = snapshot.track.as_ref();
        let loaded = is_ready(snapshot);
        // Both follow the speed of the deck, which sync sets apart from the fader.
        let offset = format::signed_percent(snapshot.speed - 1.0);
        // The tempo of the grid where the deck plays; until there is one, the tempo the tag
        // gives.
        let base = snapshot
            .grid
            .bpm_at(snapshot.position_seconds)
            .map(|bpm| bpm.value())
            .or_else(|| track.and_then(|track| track.tags.bpm))
            .filter(|_| loaded);
        let (tempo, detail) = match base {
            Some(base) => (
                format::bpm(base * snapshot.speed),
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
        let none = texts.get("deck.no_time");
        let (large, second, third, large_ink) = match track {
            Some(track) => {
                let elapsed = snapshot.position_seconds.min(track.seconds);
                let left = (track.seconds - elapsed).max(0.0);
                let remaining = format::time(left, true);
                let elapsed = format::time(elapsed, false);
                let total = format::time(track.seconds, false);
                let ending = snapshot.playing && left < ENDING_SECONDS;
                if remaining_large {
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
        Self {
            tempo,
            detail,
            tempo_ink,
            large,
            second,
            third,
            large_ink,
            small_ink: if loaded {
                color.text_secondary
            } else {
                color.text_disabled
            },
        }
    }
}

/// CUE, CUP and play or pause, along the bottom of `area`.
/// The transport keys from the left of `row`, as tall as it, each at least `key_width` wide.
/// Returns where the last key ends.
fn transport(
    cx: &mut Cx<'_>,
    row: Rect,
    key_width: f32,
    deck: DeckId,
    snapshot: &DeckSnapshot,
) -> f32 {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let loaded = is_ready(snapshot);
    let lit = deck_color(theme, deck);
    let letter = deck.letter().to_string();
    let height = row.height();
    let top = row.top();
    let mut left = row.left();
    // Narrow keys, as in the decks of the four-deck layout, sit closer together and keep
    // less room around their labels.
    let narrow = key_width < tokens.size.button_transport;
    let gap = if narrow {
        tokens.space.s2
    } else {
        tokens.space.s3
    };
    // CUE, CUP and play; the four-deck layout puts play first, as its screen orders them.
    let cue = texts.get("deck.cue");
    let cup = texts.get("deck.cup");
    let widths = [
        label_width(cx, cue, key_width, narrow),
        label_width(cx, cup, key_width, narrow),
        if narrow { NARROW_PLAY_WIDTH } else { key_width },
    ];
    let order = if narrow { [2, 0, 1] } else { [0, 1, 2] };
    let mut slots = [Rect::NOTHING; 3];
    for index in order {
        slots[index] =
            Rect::from_min_size(egui::pos2(left, top), egui::vec2(widths[index], height));
        left += widths[index] + gap;
    }

    let rect = slots[0];
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

    let rect = slots[1];
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

    let rect = slots[2];
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
        keeps_led: false,
        text: None,
    };
    key::key(
        cx,
        rect,
        id(("deck-play", deck.index())),
        spec,
        Press::Click(Command::DeckPlayPause { deck }.into()),
    );

    let flux = texts.get("deck.flux");
    let rect = Rect::from_min_size(
        egui::pos2(left, top),
        egui::vec2(
            label_width(cx, flux, FLUX_WIDTH.min(key_width), narrow),
            height,
        ),
    );
    left = rect.right() + gap;
    let name = texts.format("deck.flux_name", &[("deck", &letter)]);
    let mut spec = KeySpec::text(flux)
        .enabled(loaded)
        .lit(snapshot.flux.then_some(tokens.color.text_primary));
    spec.name = &name;
    key::key(
        cx,
        rect,
        id(("deck-flux", deck.index())),
        spec,
        Press::Click(Command::DeckFluxToggle { deck }.into()),
    );
    let rect = Rect::from_min_size(
        egui::pos2(left, top),
        egui::vec2(
            if narrow {
                NARROW_REVERSE_WIDTH
            } else {
                REVERSE_WIDTH
            },
            height,
        ),
    );
    let name = texts.format("deck.reverse_name", &[("deck", &letter)]);
    let mut spec = KeySpec::icon(Glyph::Reverse, 14.0, &name)
        .enabled(loaded)
        .lit(snapshot.reverse.then_some(tokens.color.text_primary));
    spec.held = Some(tokens.color.text_primary);
    key::key(
        cx,
        rect,
        id(("deck-reverse", deck.index())),
        spec,
        Press::Hold {
            down: Command::DeckReverse {
                deck,
                pressed: true,
            }
            .into(),
            up: Command::DeckReverse {
                deck,
                pressed: false,
            }
            .into(),
        },
    );
    rect.right()
}

/// The width of a key for its label: [`key::width_for`], or with half the room around the
/// label for the narrow keys of the four-deck layout.
fn label_width(cx: &Cx<'_>, label: &str, minimum: f32, narrow: bool) -> f32 {
    if !narrow {
        return key::width_for(cx, label, minimum);
    }
    let tokens = &cx.theme.tokens;
    let text = paint::text_width(cx.painter(), cx.theme, label, tokens.text.button);
    (text + 2.0 * tokens.space.s2).ceil().max(minimum)
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
    let narrow = column.width() < 64.0;
    let width = if narrow {
        label_width(cx, &label, 0.0, true).min(column.width())
    } else {
        key::width_for(cx, &label, 0.0).max(43.0)
    };
    let key_height = if narrow {
        NARROW_TEMPO_KEY_HEIGHT
    } else {
        24.0
    };
    let range_key = Rect::from_min_size(
        egui::pos2(column.center().x - width / 2.0, column.top()),
        egui::vec2(width, key_height),
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

    let nudge_top = column.bottom() - key_height - 2.0;
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
            look: TEMPO_FADER,
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
        // Side by side across the narrow column; centred under the fader in a wide one.
        let rect = if narrow {
            let width = (column.width() - tokens.space.s1) / 2.0;
            Rect::from_min_size(
                egui::pos2(
                    column.left() + (width + tokens.space.s1) * index as f32,
                    nudge_top,
                ),
                egui::vec2(width, key_height),
            )
        } else {
            Rect::from_min_size(
                egui::pos2(column.center().x - 30.0 + 32.0 * index as f32, nudge_top),
                egui::vec2(28.0, 24.0),
            )
        };
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
        DeckNotice::StemsFailed => ("deck.stems_failed_title", "deck.stems_failed_text"),
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
